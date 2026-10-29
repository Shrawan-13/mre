# Fonts

`Roboto-Regular.ttf` is Roboto, licensed under the Apache License 2.0
(https://www.apache.org/licenses/LICENSE-2.0). It is vendored for tests,
examples and deterministic golden images so that text metrics do not depend on
which fonts a machine happens to have installed.

Production apps should load their own fonts (or the system font set) through
`mre-text::TextEngine`; the bundled file is not linked into the engine binary.
