# Inter

Static cuts of Inter 4.001 (<https://github.com/rsms/inter>, SIL OFL 1.1, see
`Inter-LICENSE.txt`), made from upstream `InterVariable.ttf` with
`instance.py` (fontTools): the text cut (`opsz` 14) at 400, 500, 600 and 700,
and the display cut (`opsz` 32) at 700 as the family "Inter Display".
Static files because GPUI's Linux text system loads every face at its
default instance, so a variable font would render one weight only.

```sh
uv run --with fonttools python instance.py   # next to InterVariable.ttf
```
