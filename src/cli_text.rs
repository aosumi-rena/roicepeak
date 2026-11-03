//! Explicit-path Japanese text commands for the standalone executable.
use roicepeak::{Error, Result, japanese::Analysis, synthesis::Render};
use std::{fs::OpenOptions, io::Write, path::Path};

fn analyze(dic: &str, input: &str) -> Result<(String, Analysis)> {
    let bytes = super::read(input, 65_536)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| Error("Japanese text input must be UTF-8".into()))?;
    // Normalization includes number expansion; applying it twice changes text.
    let tagger = roicepeak::frontend::Tagger::open(Path::new(dic), &[])?;
    let excluded = tagger.normalization_exclusions(text, None)?;
    let normalized = roicepeak::japanese::normalize_excluding(text, &excluded)?;
    let morphs = tagger.parse(&normalized)?;
    let mut analysis = roicepeak::japanese::analyze(&morphs)?;
    analysis.normalization_exclusions = excluded;
    Ok((normalized, analysis))
}

fn write_analysis(directory: &Path, normalized: &str, analysis: &Analysis) -> Result<()> {
    let mut source = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("normalized.txt"))?;
    source.write_all(normalized.as_bytes())?;
    source.sync_all()?;
    let mut tokens = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("tokens.tsv"))?;
    let mut mask = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("prediction.tsv"))?;
    for (t, token) in analysis.tokens.iter().enumerate() {
        for (s, syllable) in token.syllables.iter().enumerate() {
            if [&token.text, &syllable.reading]
                .iter()
                .any(|v| v.contains(['\t', '\r', '\n']))
            {
                return Err(Error("token fields contain TSV delimiters".into()));
            }
            writeln!(mask, "{t}\t{s}\t{}", u8::from(analysis.predict[t][s]))?;
            for phone in &syllable.phones {
                writeln!(
                    tokens,
                    "{t}\t{}\t{}\t{}\t{s}\t{}\t{}\t{}\t{}",
                    token.text,
                    token.language,
                    token.pos,
                    syllable.reading,
                    syllable.code,
                    phone.symbol,
                    u8::from(phone.nucleus)
                )?;
            }
        }
    }
    tokens.sync_all()?;
    mask.sync_all()?;
    Ok(())
}

fn wave(path: &Path, pcm: &roicepeak::tensor::Tensor) -> Result<()> {
    let mut output = OpenOptions::new().write(true).create_new(true).open(path)?;
    roicepeak::wave::write(&mut output, pcm, 48_000, roicepeak::wave::Encoding::Float32)?;
    output.sync_all()?;
    Ok(())
}

fn write_render(mut render: Render, stage: &str, output: &str) -> Result<()> {
    if stage == "all" {
        let directory = Path::new(output);
        if let Some(pcm) = render.stages.get("pcm") {
            wave(&directory.join("audio.wav"), pcm)?;
        }
        render
            .stages
            .insert("linguistic", render.encoded.linguistic);
        for (name, tensor) in render.stages {
            super::write_tensor(
                &directory.join(format!("{name}.rmat")).to_string_lossy(),
                &tensor,
            )?;
        }
    } else if stage == "wav" {
        wave(
            Path::new(output),
            render
                .stages
                .get("pcm")
                .ok_or_else(|| Error("PCM stage unavailable".into()))?,
        )?;
    } else {
        let tensor = if stage == "linguistic" {
            render.encoded.linguistic
        } else {
            render
                .stages
                .remove(stage)
                .ok_or_else(|| Error("stage unavailable for this model".into()))?
        };
        super::write_tensor(output, &tensor)?;
    }
    Ok(())
}

pub(crate) fn execute(args: &[String]) -> Option<Result<bool>> {
    match args {
        [cmd, dic, input, output] if cmd == "analyze-japanese" => Some((|| {
            let (normalized, analysis) = analyze(dic, input)?;
            std::fs::create_dir(output)?;
            write_analysis(Path::new(output), &normalized, &analysis)?;
            println!(
                "{{\"tokens\":{},\"normalized_bytes\":{}}}",
                analysis.tokens.len(),
                normalized.len()
            );
            Ok(true)
        })()),
        [cmd, manifest, dic, input, stage, output] if cmd == "render-text" => Some((|| {
            if ![
                "linguistic",
                "language",
                "prosody",
                "controls",
                "pitch",
                "physical",
                "pcm",
                "prediction_mask",
                "wav",
                "all",
            ]
            .contains(&stage.as_str())
            {
                return Err(Error("unknown text synthesis stage".into()));
            }
            let (normalized, analysis) = analyze(dic, input)?;
            let model = roicepeak::manifest::Manifest::read(Path::new(manifest))?;
            let mut budget = roicepeak::graph::Budget {
                multiply_adds: 100_000_000_000,
                output_elements: 512 * 1024 * 1024,
            };
            let rendered =
                model.render_with_prediction(&analysis.tokens, &analysis.predict, &mut budget)?;
            if stage == "all" {
                std::fs::create_dir(output)?;
                write_analysis(Path::new(output), &normalized, &analysis)?;
            }
            write_render(rendered, stage, output)?;
            Ok(true)
        })()),
        _ => None,
    }
}
