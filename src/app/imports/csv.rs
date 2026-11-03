use super::{Batch, CsvCatalog, ImportedBlock, MAX_BLOCKS, MAX_TEXT, line_error, lines};

pub(super) fn parse(text: &str, catalog: &CsvCatalog) -> Result<Batch, String> {
    if catalog.narrators.len() > 1024 || catalog.presets.len() > 10_000 {
        return Err("CSV narrator/preset catalog exceeds its bounds".into());
    }
    let mut blocks = Vec::new();
    for (line_index, line) in lines(text)?.into_iter().enumerate() {
        let source_line = line_index + 1;
        // Quotes and backslash escapes deliberately remain ordinary characters.
        let mut cells = Vec::new();
        for cell in line
            .split([',', '\t'])
            .filter(|cell| !cell.trim().is_empty())
        {
            if cells.len() >= 4096 {
                return Err(line_error(
                    source_line,
                    "CSV row exceeds 4096 nonempty cells",
                ));
            }
            if cell.len() > MAX_TEXT {
                return Err(line_error(source_line, "CSV speech cell exceeds 1 MiB"));
            }
            cells.push(cell);
        }
        if cells.is_empty() {
            continue;
        }
        let named = catalog.narrators.contains(cells[0].trim());
        let narrator = if named {
            Some(cells.remove(0).trim().to_owned())
        } else {
            catalog.default_narrator.clone()
        };
        let preset = narrator
            .as_ref()
            .and_then(|name| {
                cells
                    .last()
                    .and_then(|cell| catalog.presets.get(&(name.clone(), cell.trim().to_owned())))
            })
            .cloned();
        if preset.is_some() {
            cells.pop();
        }
        for cell in cells {
            if blocks.len() >= MAX_BLOCKS {
                return Err(line_error(source_line, "more than 10000 CSV speech cells"));
            }
            blocks.push(ImportedBlock {
                text: cell.to_owned(),
                narrator: narrator.clone(),
                start: None,
                parameters: preset.clone(),
                source_line,
            });
        }
    }
    if blocks.is_empty() {
        return Err("CSV input contains no speech cells".into());
    }
    let warnings = if catalog.presets.is_empty() {
        vec![
            "No narrator preset catalog is configured; trailing CSV cells remain speech text."
                .into(),
        ]
    } else {
        Vec::new()
    };
    Ok(Batch { blocks, warnings })
}
