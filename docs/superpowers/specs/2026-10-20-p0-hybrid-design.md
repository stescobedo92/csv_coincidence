# Diseño P0 — Streaming, reglas por columna, PII y span/capturas

Audiencias: (A) usuarios CLI de limpieza de datos y (B) desarrolladores Rust.
Principio: cada función nace en el core de la librería y se expone en la CLI.
Compatibilidad: cambios aditivos; las APIs existentes conservan firma y
comportamiento salvo `MatchHit`, que gana campos nuevos con `#[serde(default)]`.

## 1. Span y capturas en `MatchHit`

- Nuevos campos: `matched_text: String`, `start: usize`, `end: usize`
  (offsets de bytes UTF-8 sobre el valor de la celda) y
  `captures: Vec<Option<String>>` (grupos 1..N en modo regex).
- En modo regex se usa `captures()` para obtener span del grupo 0 y capturas.
- En modo contains, el span es la primera ocurrencia (case-insensitive si se
  pide); `captures` va vacío.
- En modo fuzzy, el span cubre la celda completa; `score` sigue poblado.
- Las exportaciones CSV/TSV añaden columnas `matched_text,start,end`.
  La de texto incluye el fragmento. JSON incluye todos los campos vía serde.

## 2. API streaming

- `MatchHitIter<R: Read>` implementa `Iterator<Item = Result<MatchHit>>` leyendo
  registro a registro con `read_record`, sin materializar todos los hits.
- Constructores: `find_matches_iter_reader`, `find_matches_iter` (ruta) y
  `find_matches_iter_from_read`. Validan patrones/reglas y columnas al crear.
- `find_matches_reader` y `count_matches_reader` se reimplementan sobre el
  iterador; `count` ya no construye un `Vec`.
- Variantes con escritor: `filter_rows_to_writer`, `replace_matches_to_writer`
  y `dedupe_rows_to_writer` escriben a cualquier `Write` y devuelven el número
  de filas escritas. Las variantes que devuelven `String` delegan en ellas.
- La CLI usa el iterador para `count` y las variantes `Write` para
  `filter/replace/merge/dedupe` escribiendo directo a stdout/fichero.

## 3. Reglas por columna AND/OR/NOT

- `ColumnRule { columns, patterns, mode, case_insensitive, negated }` y
  `LogicOp::{And, Or}` en `SearchOptions` vía `column_rules: Vec<ColumnRule>`
  y `logic: LogicOp`. Vector vacío = comportamiento legacy (los campos
  globales se normalizan internamente a una única regla no negada).
- Una regla se satisface en una fila si alguna celda seleccionada coincide con
  algún patrón; si `negated`, se satisface cuando ninguna coincide.
  La fila cumple si todas (And) o alguna (Or) regla se satisface.
- Semántica por operación:
  - `find/count/report`: solo se emiten hits de reglas no negadas y solo de
    filas que cumplen la condición combinada.
  - `filter`: la fila se conserva si cumple la condición combinada.
  - `replace`: se reemplaza en celdas de reglas no negadas (secuencial);
    `only_matched_rows` sigue gobernado por si hubo cambios.
  - `dedupe`: el fingerprint usa celdas que coinciden con reglas no negadas.
- CLI: `--where '[!]columna=patrón'` repetible (nombre de columna antes del
  primer `=`; `!` inicial niega) y `--logic and|or`. El modo y
  `--ignore-case` globales se heredan en cada regla CLI.
  Con `--where`, `-p` deja de ser obligatorio.

## 4. Biblioteca PII, validadores y fichero de patrones

- Módulo `patterns`: constantes y `builtin_pattern(name)` /
  `builtin_patterns()` para `email`, `url`, `ipv4`, `iban`, `credit-card`,
  `dni`, `nie`, `phone-es`, `date-iso`, `uuid`. Son patrones sin anclar para
  localizar el dato dentro de la celda (el span lo delimita).
- Validadores exactos: `is_valid_credit_card` (Luhn), `is_valid_iban`
  (módulo 97), `is_valid_dni` e `is_valid_nie` (letra de control).
- `parse_patterns(text)` y `read_pattern_file(path)`: un patrón por línea,
  se ignoran líneas vacías y comentarios `#`.
- CLI: `--builtin NAME` repetible y `--pattern-file PATH` se fusionan con
  `-p`. `--pattern-file -` es error (conflicto con stdin de datos).
- Validación extra: el threshold fuzzy debe estar en `0.0..=1.0`.

## Errores y tests

- Errores existentes de `CsvCoincidenceError` para columna no encontrada,
  índice inválido, patrones vacíos y umbral fuzzy inválido (mensaje).
- Tests duraderos en `tests/p0_features.rs`: iterador vs. `find`, escritores,
  span/capturas (regex/contains/fuzzy), reglas AND/OR/NOT, builtins y
  validadores, parseo de fichero de patrones. Gates CI: `fmt`, `clippy -D
  warnings`, `test --all-features` y smoke CLI.
