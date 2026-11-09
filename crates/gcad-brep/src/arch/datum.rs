use super::{Building, Grid, Level};
use crate::Model;
use crate::parse::{Line, eval, eval_point};
use anyhow::{Context, Result, anyhow, bail};

impl Model {
    pub(crate) fn op_units(&mut self, line: &Line) -> Result<String> {
        match line.positional.as_slice() {
            [unit] if unit == "mm" && line.named.is_empty() => {
                self.arch.units = Some(unit.clone());
                Ok("lengths are in mm".into())
            }
            _ => bail!("only `units mm` is supported; lengths are millimetres"),
        }
    }

    pub(crate) fn op_building(&mut self, line: &Line) -> Result<String> {
        let name = match line.positional.as_slice() {
            [name] => name.clone(),
            _ => bail!("write `building \"Name\" north=0 datum=0`"),
        };
        let mut building = Building {
            name,
            ..Default::default()
        };
        for (key, text) in &line.named {
            let value = eval(text, &self.scope).with_context(|| format!("`{key}`"))?;
            match key.as_str() {
                "north" if (-360.0..=360.0).contains(&value) => building.north = value,
                "north" => bail!("north={value} must be an angle from -360 to 360 degrees"),
                "datum" => building.datum = value,
                other => bail!("`building` takes north= and datum=, not `{other}=`"),
            }
        }
        let text = format!(
            "building \"{}\", north {}°, datum {} mm",
            building.name,
            crate::parse::label(building.north),
            crate::parse::label(building.datum)
        );
        self.arch.building = Some(building);
        Ok(text)
    }

    pub(crate) fn op_level_datum(&mut self, line: &Line) -> Result<String> {
        let (name, height, title) = match line.positional.as_slice() {
            [name, height] => (name, height, String::new()),
            [name, height, title] => (name, height, title.clone()),
            _ => bail!("write `level L1 2700 \"First\"`"),
        };
        if !line.named.is_empty() {
            bail!("`level` takes a name, a height and a title");
        }
        if self.arch.levels.iter().any(|l| l.name == *name) {
            bail!("there is already a level `{name}`");
        }
        if self.scope.contains_key(name) {
            bail!("`{name}` is already a value; levels need their own names");
        }
        let z = eval(height, &self.scope).with_context(|| format!("height of `{name}`"))?;
        self.scope.insert(name.clone(), z);
        self.arch.levels.push(Level {
            name: name.clone(),
            z,
            title: title.clone(),
        });
        Ok(format!(
            "level {name} at {}{}",
            crate::parse::label(z),
            if title.is_empty() {
                String::new()
            } else {
                format!(" \"{title}\"")
            }
        ))
    }

    pub(crate) fn op_grid(&mut self, line: &Line) -> Result<String> {
        let [name] = line.positional.as_slice() else {
            bail!("write `grid A x=0` or `grid 1 y=0`");
        };
        let (axis, text) = match line.named.as_slice() {
            [(axis, text)] if axis == "x" || axis == "y" => (axis.as_str(), text),
            _ => bail!("a grid line takes one of x= or y=, like `grid A x=0`"),
        };
        let offset = eval(text, &self.scope)?;
        self.add_grid(name, axis == "x", offset)?;
        Ok(format!(
            "grid {name} at {axis}={}",
            crate::parse::label(offset)
        ))
    }

    fn add_grid(&mut self, name: &str, along_x: bool, offset: f64) -> Result<()> {
        let good = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
        if !good {
            bail!("grid names are capital letters or numbers, not `{name}`");
        }
        if self.arch.grids.iter().any(|g| g.name == name) {
            bail!("there is already a grid `{name}`");
        }
        let grid = Grid {
            name: name.to_string(),
            along_x,
            offset,
        };
        for other in self.arch.grids.iter().filter(|g| g.along_x != along_x) {
            let (x, y) = if along_x {
                (&grid, other)
            } else {
                (other, &grid)
            };
            let crossing = format!("{}{}", x.name, y.name);
            self.scope.insert(format!("{crossing}.x"), x.offset);
            self.scope.insert(format!("{crossing}.y"), y.offset);
        }
        self.arch.grids.push(grid);
        Ok(())
    }

    pub(crate) fn op_gridfile(&mut self, line: &Line) -> Result<String> {
        let [file] = line.positional.as_slice() else {
            bail!("write `gridfile grids.csv` with rows name,axis,offset");
        };
        let path = self.relative(file);
        let text = crate::host::read_to_string(&path)
            .map_err(|e| anyhow!("reading {}: {e}", path.display()))?;
        let mut count = 0;
        for (number, row) in text.lines().enumerate() {
            let row = row.trim();
            if row.is_empty() || row.starts_with('#') {
                continue;
            }
            let cells: Vec<&str> = row.split(',').map(str::trim).collect();
            let place = || format!("{}:{}", path.display(), number + 1);
            let [name, axis, offset] = cells.as_slice() else {
                bail!("{}: a row is name,axis,offset", place());
            };
            if *name == "name" && number == 0 {
                continue;
            }
            let along_x = match *axis {
                "x" => true,
                "y" => false,
                other => bail!("{}: axis is x or y, not `{other}`", place()),
            };
            let offset = eval(offset, &self.scope).with_context(place)?;
            self.add_grid(name, along_x, offset).with_context(place)?;
            count += 1;
        }
        Ok(format!("{count} grid lines from {}", path.display()))
    }

    pub(crate) fn plan_point(&self, word: &str) -> Result<(f64, f64)> {
        if !word.contains(',') && !self.scope.contains_key(&format!("{word}.x")) {
            let names: Vec<&str> = self.arch.grids.iter().map(|g| g.name.as_str()).collect();
            bail!(
                "no grid crossing or point `{word}`; grids are {}",
                if names.is_empty() {
                    "none yet".to_string()
                } else {
                    names.join(", ")
                }
            );
        }
        eval_point(word, &self.scope)
    }
}
