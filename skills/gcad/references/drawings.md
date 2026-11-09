# Drawings

A `.gdwg` file draws a part or assembly: `part file` once, `title`, then per page a `sheet`, its
`view`s and `section`s, dimensions, labels and notes. Check with `gcad check x.gdwg`, write with
`gcad export x.gdwg x.pdf` (`.svg` writes one file per sheet), then `pdftoppm -png -r 70 x.pdf
/tmp/x` and look at every page: a clean check does not mean it reads well. A second `part` line
swaps the model for the next sheet.

```text
part ../parts/shaft.gcad d=12 L=131
title "El shaft, 1 off, Ø12 silver steel"
sheet "El shaft" size=A3 scale=3:2
view from=-y place=40,40 label="front"
dim -6,0,0 -6,0,131 offset=14
section from=-y at=0 place=130,40 label="section A-A"
label main:keyway.side side=right
label main:tap0.side
note "Ø12 silver steel, 131 long|Break all edges 0.5" place=20,20
```

- **Sheet.** `size=A4..A0` landscape; the title block (bottom right) is an obstacle. Use the
  largest `scale=` at which each view fits (166 mm at 2:1 needs A3); the error says what did not
  fit. A view can take its own `scale=`.
- **Views.** `view from=-y|+y|-x|+x|+z|-z` with part patterns as in `gcad bom` (`*`, `!pv.*`,
  `profile=`). `section from=-y at=0` cuts square to the view. `place=x,y` is the lower left
  corner in mm on the sheet; omit it to flow views left to right. `window=u0,v0,u1,v1` crops.
- **Labels.** The default annotation. `label target` or `label "text" target` and gcad places the
  text and leader, so never write coordinates for text. A target is `body:faces` (a hole writes
  its own `⌀12`, `4× ⌀3.4` or `M3 tapped (⌀2.5 drill)`), `body:a&b` (an edge) or `x,y,z`. The
  body is a body name in the part file (`main` for a plain part) or a part in the assembly;
  groups are the part file's labels (`bore.side`). Several targets share one text.
- `side=left|right|top|bottom` picks the label column. On "no room beside the view", shrink the
  scale, `place` views further apart or use a bigger sheet.
- **Dimensions.** `dim x,y,z x,y,z offset=N` between model points in the last view, `N` mm outside
  the part (positive puts a horizontal dimension below, a vertical one left). Use edge points on
  the near side and stack with `offset=12`, then `24`. `text="..."` overrides the value.
  `dims parts along=h|v of=ends|edges|centres` chains parts of an assembly; `style=ordinate`,
  `level` (heights on an elevation), `cutlist` and `nest` (panels on stock with saw cuts) exist too.
- **Notes.** `note "line one|line two" place=x,y` for material, finish and tolerances (an error if
  it sits on a view). Keep a made part to one sheet: views, the dimensions a machinist needs,
  one label per feature, a note for the rest. Put the quantity in the title (`"Worm shaft, 2 off"`).
- **Long parts.** One that lies along z draws vertically. Make a one-line `.gasm` (`part p
  ../parts/x.gcad`, `rotate p 90 axis=y`), draw that, and label with `p:feature.side`.
- PDF text is Latin-1: `Ø ⌀ × ° ±` print, other symbols become `?`.
