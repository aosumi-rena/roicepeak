use roicepeak::{
    Error, Result,
    compare::compare,
    dnni,
    graph::Graph,
    language::LanguageModel,
    normalization::Normalizer,
    tensor::{MAX_FILE_BYTES, Tensor},
};
mod cli_text;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

fn read(path: &str, cap: usize) -> Result<Vec<u8>> {
    let file = File::open(Path::new(path))?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > cap as u64 {
        return Err(Error("input must be a bounded regular file".into()));
    }
    let mut bytes = Vec::new();
    file.take(cap as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > cap {
        return Err(Error("input grew beyond budget".into()));
    }
    Ok(bytes)
}
fn select<'a, 'b>(root: &'b dnni::Node<'a>, path: &str) -> Result<&'b dnni::Node<'a>> {
    let mut node = root;
    if path == "root" {
        return Ok(node);
    }
    for part in path.split('/') {
        let i = part
            .parse::<usize>()
            .map_err(|_| Error("node path must contain child indices".into()))?;
        node = node
            .children
            .get(i)
            .ok_or_else(|| Error("node path outside tree".into()))?;
    }
    Ok(node)
}
fn write_tensor(output: &str, y: &Tensor) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    file.write_all(&y.to_rmat())?;
    file.sync_all()?;
    println!(
        "{{\"rows\":{},\"columns\":{},\"elements\":{},\"finite\":true}}",
        y.rows(),
        y.cols(),
        y.values().len()
    );
    Ok(())
}
fn infer(
    model: &str,
    path: &str,
    input: &str,
    condition: Option<&str>,
    output: &str,
) -> Result<bool> {
    let bytes = read(model, dnni::MAX_BYTES)?;
    let root = dnni::parse(&bytes)?;
    let graph = Graph::compile(select(&root, path)?)?;
    let x = Tensor::from_rmat(&read(input, MAX_FILE_BYTES)?)?;
    let y = if let Some(c) = condition {
        let c = Tensor::from_rmat(&read(c, MAX_FILE_BYTES)?)?;
        graph.run_conditioned(&x, &c, &mut Default::default())?
    } else {
        graph.run(&x)?
    };
    write_tensor(output, &y)?;
    Ok(true)
}
fn execute(args: &[String]) -> Result<bool> {
    if let Some(result) = cli_text::execute(args) {
        return result;
    }
    match args {
        [cmd, dic, input, output] if cmd == "tokenize" => {
            let bytes = read(input, 65_536)?;
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| Error("lexical input must be UTF-8".into()))?;
            let tagger = roicepeak::frontend::Tagger::open(Path::new(dic), &[])?;
            let morphs = tagger.parse(text)?;
            let escape = |s: &str| {
                s.replace('\\', "\\\\")
                    .replace('\t', "\\t")
                    .replace('\r', "\\r")
                    .replace('\n', "\\n")
            };
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(output)?;
            writeln!(file, "start\tend\tsurface\tfeature\tlc\trc\tcost\tunknown")?;
            for morph in &morphs {
                writeln!(
                    file,
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    morph.start,
                    morph.end,
                    escape(&morph.surface),
                    escape(&morph.feature),
                    morph.lc,
                    morph.rc,
                    morph.cost,
                    u8::from(morph.unknown)
                )?;
            }
            file.sync_all()?;
            println!(
                "{{\"morphs\":{},\"input_bytes\":{}}}",
                morphs.len(),
                text.len()
            );
            Ok(true)
        }
        [cmd, manifest, input, stage, output] if cmd == "render-phones" => {
            if ![
                "linguistic",
                "language",
                "prosody",
                "controls",
                "pitch",
                "physical",
                "pcm",
                "all",
            ]
            .contains(&stage.as_str())
            {
                return Err(Error("unknown synthesis stage".into()));
            }
            let model = roicepeak::manifest::Manifest::read(Path::new(manifest))?;
            let text = read(input, 16 * 1024 * 1024)?;
            let tokens = roicepeak::phonemes::parse_tsv(
                std::str::from_utf8(&text).map_err(|_| Error("token TSV must be UTF-8".into()))?,
            )?;
            let mut budget = roicepeak::graph::Budget {
                multiply_adds: 100_000_000_000,
                output_elements: 512 * 1024 * 1024,
            };
            let mut rendered = model.render(&tokens, &mut budget)?;
            if stage == "all" {
                std::fs::create_dir(output)?;
                rendered
                    .stages
                    .insert("linguistic", rendered.encoded.linguistic);
                for (name, tensor) in rendered.stages {
                    write_tensor(
                        &Path::new(output)
                            .join(format!("{name}.rmat"))
                            .to_string_lossy(),
                        &tensor,
                    )?;
                }
                return Ok(true);
            }
            let out = if stage == "linguistic" {
                rendered.encoded.linguistic
            } else {
                rendered
                    .stages
                    .remove(stage.as_str())
                    .ok_or_else(|| Error("stage unavailable for this model generation".into()))?
            };
            write_tensor(output, &out)?;
            Ok(true)
        }
        [cmd, model, weights, offsets, phonemes, output] if cmd == "mix-style" => {
            let bytes = read(model, dnni::MAX_BYTES)?;
            let root = dnni::parse(&bytes)?;
            let weights = Tensor::from_rmat(&read(weights, MAX_FILE_BYTES)?)?;
            if weights.rows() != 1 {
                return Err(Error("style weights require one row".into()));
            }
            let offsets = if offsets == "-" {
                None
            } else {
                let fields: Vec<_> = offsets
                    .split(',')
                    .map(|s| {
                        s.parse::<usize>()
                            .map_err(|_| Error("invalid stylebook offset".into()))
                    })
                    .collect::<Result<_>>()?;
                if fields.len() != 5 {
                    return Err(Error(
                        "stylebook offsets: bap,volume,intonation,speed,tension; 0 means absent"
                            .into(),
                    ));
                }
                let o = |i: usize| (fields[i] != 0).then_some(fields[i]);
                Some(roicepeak::stylebook::Offsets {
                    aperiodicity: o(0),
                    volume: o(1),
                    intonation: o(2),
                    speed: o(3),
                    tension: o(4),
                })
            };
            let phones = if phonemes == "-" {
                Vec::new()
            } else {
                String::from_utf8(read(phonemes, 1024 * 1024)?)
                    .map_err(|_| Error("phoneme list must be UTF-8".into()))?
                    .lines()
                    .map(str::to_owned)
                    .collect()
            };
            let book = roicepeak::stylebook::Stylebook::compile(&root, offsets, phones)?;
            let y = book.vector(weights.values())?;
            write_tensor(output, &Tensor::new(1, y.len(), y)?)?;
            Ok(true)
        }
        [cmd, model, path, index, output] if cmd == "speaker-vector" => {
            let bytes = read(model, dnni::MAX_BYTES)?;
            let root = dnni::parse(&bytes)?;
            let index = index
                .parse::<usize>()
                .map_err(|_| Error("invalid speaker index".into()))?;
            let y = roicepeak::stylebook::speaker_vector(select(&root, path)?, index)?;
            write_tensor(output, &Tensor::new(1, y.len(), y)?)?;
            Ok(true)
        }
        [cmd, model, input, output] if cmd == "encode-phones" || cmd == "phone-refs" => {
            let bytes = read(model, dnni::MAX_BYTES)?;
            let encoder = roicepeak::phonemes::PhonemeEncoder::compile(&dnni::parse(&bytes)?)?;
            let text = read(input, 16 * 1024 * 1024)?;
            let tokens = roicepeak::phonemes::parse_tsv(
                std::str::from_utf8(&text).map_err(|_| Error("token TSV must be UTF-8".into()))?,
            )?;
            let encoded = encoder.encode(&tokens)?;
            let out = if cmd == "encode-phones" {
                encoded.linguistic
            } else {
                Tensor::new(
                    encoded.references.len(),
                    7,
                    encoded
                        .references
                        .iter()
                        .enumerate()
                        .flat_map(|(i, r)| {
                            [
                                r[0] as f32,
                                r[1] as f32,
                                r[2] as f32,
                                r[3] as f32,
                                r[4] as f32,
                                f32::from(encoded.nucleus[i]),
                                encoded.codes[i] as f32,
                            ]
                        })
                        .collect(),
                )?
            };
            write_tensor(output, &out)?;
            Ok(true)
        }
        [
            cmd,
            im,
            accent,
            input,
            lm,
            mapping,
            codes,
            speaker_im,
            speaker_accent,
            style,
            format,
            output,
        ] if cmd == "prosody" => {
            let im_bytes = read(im, dnni::MAX_BYTES)?;
            let accent_bytes = read(accent, dnni::MAX_BYTES)?;
            let fields: Vec<usize> = format
                .split(',')
                .map(|p| {
                    p.parse::<usize>().map_err(|_| {
                        Error(
                            "prosody format must contain Japanese ID and eleven flag slots".into(),
                        )
                    })
                })
                .collect::<Result<_>>()?;
            if fields.len() != 12 || fields[0] > u32::MAX as usize {
                return Err(Error("prosody format needs twelve fields".into()));
            }
            let format = roicepeak::prosody::FeatureFormat {
                japanese: fields[0] as u32,
                slots: fields[1..].try_into().unwrap(),
            };
            let model = roicepeak::prosody::Prosody::compile(
                &dnni::parse(&im_bytes)?,
                &dnni::parse(&accent_bytes)?,
                format,
            )?;
            let tensor = |p: &str| Tensor::from_rmat(&read(p, MAX_FILE_BYTES)?);
            let x = tensor(input)?;
            let lm = tensor(lm)?;
            let map = tensor(mapping)?;
            if map.cols() != 1
                || map
                    .values()
                    .iter()
                    .any(|v| *v < 0.0 || v.fract() != 0.0 || *v >= lm.rows() as f32)
            {
                return Err(Error("invalid word mapping".into()));
            }
            let words: Vec<_> = map.values().iter().map(|v| *v as usize).collect();
            let edited = if codes == "-" {
                None
            } else {
                let c = tensor(codes)?;
                if c.cols() != 1
                    || c.values()
                        .iter()
                        .any(|v| *v < 0.0 || v.fract() != 0.0 || *v > 65535.0)
                {
                    return Err(Error("invalid prosody codes".into()));
                }
                Some(c.values().iter().map(|v| *v as u32).collect::<Vec<_>>())
            };
            let si = tensor(speaker_im)?;
            let sa = tensor(speaker_accent)?;
            let style = tensor(style)?;
            if si.rows() != 1 || sa.rows() != 1 || style.rows() != 1 {
                return Err(Error("prosody conditioning must have one row".into()));
            }
            let mut budget = roicepeak::graph::Budget {
                multiply_adds: 20_000_000_000,
                output_elements: 128 * 1024 * 1024,
            };
            let y = model.run(
                &x,
                &lm,
                &words,
                edited.as_deref(),
                si.values(),
                sa.values(),
                style.values(),
                &mut budget,
            )?;
            write_tensor(output, &y)?;
            Ok(true)
        }
        [cmd, input, rate, encoding, output] if cmd == "wave" => {
            let samples = Tensor::from_rmat(&read(input, MAX_FILE_BYTES)?)?;
            let rate = rate
                .parse::<u32>()
                .map_err(|_| Error("invalid WAVE sample rate".into()))?;
            let encoding = match encoding.as_str() {
                "f32" => roicepeak::wave::Encoding::Float32,
                "s16" => roicepeak::wave::Encoding::Pcm16,
                "s24" => roicepeak::wave::Encoding::Pcm24,
                _ => return Err(Error("WAVE encoding must be f32, s16 or s24".into())),
            };
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(output)?;
            roicepeak::wave::write(&mut file, &samples, rate, encoding)?;
            file.sync_all()?;
            Ok(true)
        }
        [cmd, physical, shape, gain, output] if cmd == "lf-source" => {
            let physical = Tensor::from_rmat(&read(physical, MAX_FILE_BYTES)?)?;
            let shape = Tensor::from_rmat(&read(shape, MAX_FILE_BYTES)?)?;
            let gain = gain
                .parse::<f32>()
                .map_err(|_| Error("invalid excitation gain".into()))?;
            let mut budget = roicepeak::graph::Budget {
                multiply_adds: 20_000_000_000,
                output_elements: 128 * 1024 * 1024,
            };
            write_tensor(
                output,
                &roicepeak::vocoder::lf_source(&physical, &shape, gain, &mut budget)?,
            )?;
            Ok(true)
        }
        [cmd, model, physical, stage, output] if cmd == "vocoder-stage" => {
            let bytes = read(model, dnni::MAX_BYTES)?;
            let root = dnni::parse(&bytes)?;
            let model = roicepeak::vocoder::Vocoder::compile(&root)?;
            let physical = Tensor::from_rmat(&read(physical, MAX_FILE_BYTES)?)?;
            let mut budget = roicepeak::graph::Budget {
                multiply_adds: 20_000_000_000,
                output_elements: 128 * 1024 * 1024,
            };
            let render = model.run(&physical, &mut budget)?;
            let y = render
                .stages
                .get(stage.as_str())
                .ok_or_else(|| Error("unknown vocoder stage".into()))?;
            write_tensor(output, y)?;
            Ok(true)
        }
        [
            cmd,
            model,
            linguistic,
            controls,
            style,
            speaker,
            noise,
            output,
        ] if cmd == "modern-pitch" => {
            let bytes = read(model, dnni::MAX_BYTES)?;
            let root = dnni::parse(&bytes)?;
            let model = roicepeak::modern::PitchModel::compile(&root)?;
            let linguistic = Tensor::from_rmat(&read(linguistic, MAX_FILE_BYTES)?)?;
            let controls = Tensor::from_rmat(&read(controls, MAX_FILE_BYTES)?)?;
            let style = Tensor::from_rmat(&read(style, MAX_FILE_BYTES)?)?;
            if style.rows() != 1 {
                return Err(Error("style must be one row".into()));
            }
            let speaker = speaker
                .parse::<usize>()
                .map_err(|_| Error("invalid speaker index".into()))?;
            let noise = if noise == "-" {
                None
            } else {
                Some(Tensor::from_rmat(&read(noise, MAX_FILE_BYTES)?)?)
            };
            let mut budget = roicepeak::graph::Budget {
                multiply_adds: 20_000_000_000,
                output_elements: 128 * 1024 * 1024,
            };
            write_tensor(
                output,
                &model.run(
                    &linguistic,
                    &controls,
                    style.row(0),
                    speaker,
                    noise.as_ref(),
                    &mut budget,
                )?,
            )?;
            Ok(true)
        }
        [
            cmd,
            model,
            linguistic,
            controls,
            pitch,
            style,
            speaker,
            noise,
            output,
        ] if cmd == "modern-acoustic" => {
            let bytes = read(model, dnni::MAX_BYTES)?;
            let root = dnni::parse(&bytes)?;
            let model = roicepeak::modern::AcousticDecoder::compile(&root)?;
            let linguistic = Tensor::from_rmat(&read(linguistic, MAX_FILE_BYTES)?)?;
            let controls = Tensor::from_rmat(&read(controls, MAX_FILE_BYTES)?)?;
            let pitch = Tensor::from_rmat(&read(pitch, MAX_FILE_BYTES)?)?;
            let style = Tensor::from_rmat(&read(style, MAX_FILE_BYTES)?)?;
            if style.rows() != 1 {
                return Err(Error("style must be one row".into()));
            }
            let speaker = speaker
                .parse::<usize>()
                .map_err(|_| Error("invalid speaker index".into()))?;
            let noise = if noise == "-" {
                None
            } else {
                Some(Tensor::from_rmat(&read(noise, MAX_FILE_BYTES)?)?)
            };
            let mut budget = roicepeak::graph::Budget {
                multiply_adds: 20_000_000_000,
                output_elements: 128 * 1024 * 1024,
            };
            write_tensor(
                output,
                &model.run(
                    &linguistic,
                    &controls,
                    &pitch,
                    style.row(0),
                    speaker,
                    noise.as_ref(),
                    &mut budget,
                )?,
            )?;
            Ok(true)
        }
        [cmd, plan, channels, scale, seeding, output] if cmd == "frame-noise" => {
            let text = String::from_utf8(read(plan, 1024 * 1024)?)
                .map_err(|_| Error("noise plan must be UTF-8".into()))?;
            let mut phones = Vec::new();
            let mut counts = Vec::new();
            let mut variations = Vec::new();
            for line in text.lines() {
                let fields = line.split('\t').collect::<Vec<_>>();
                if fields.len() != 3 {
                    return Err(Error(
                        "noise plan rows: phoneme TAB frames TAB variation".into(),
                    ));
                }
                phones.push(fields[0].to_owned());
                counts.push(
                    fields[1]
                        .parse::<usize>()
                        .map_err(|_| Error("invalid noise frame count".into()))?,
                );
                variations.push(
                    fields[2]
                        .parse::<i32>()
                        .map_err(|_| Error("invalid noise variation".into()))?,
                );
            }
            let channels = channels
                .parse::<usize>()
                .map_err(|_| Error("invalid noise channels".into()))?;
            let scale = scale
                .parse::<f32>()
                .map_err(|_| Error("invalid noise scale".into()))?;
            let seeds = match seeding.as_str() {
                "fixed" => Some(roicepeak::sampling::phoneme_seeds(&phones, &variations, 0)?),
                "stream" => None,
                _ => return Err(Error("noise seeding must be fixed or stream".into())),
            };
            let y = roicepeak::sampling::frame_noise(
                &counts,
                channels,
                scale,
                seeds.as_deref(),
                &mut Default::default(),
                &mut Default::default(),
            )?;
            write_tensor(output, &y)?;
            Ok(true)
        }
        [cmd, model, features, controls, style, speaker, output] if cmd == "legacy-acoustic" => {
            let bytes = read(model, dnni::MAX_BYTES)?;
            let root = dnni::parse(&bytes)?;
            let acoustic = roicepeak::acoustic::LegacyAcoustic::compile(&root)?;
            let features = Tensor::from_rmat(&read(features, MAX_FILE_BYTES)?)?;
            let controls = Tensor::from_rmat(&read(controls, MAX_FILE_BYTES)?)?;
            let style = Tensor::from_rmat(&read(style, MAX_FILE_BYTES)?)?;
            if style.rows() != 1 {
                return Err(Error("style must be one row".into()));
            }
            let speaker = speaker
                .parse::<usize>()
                .map_err(|_| Error("invalid speaker index".into()))?;
            let mut budget = roicepeak::graph::Budget {
                multiply_adds: 20_000_000_000,
                output_elements: 128 * 1024 * 1024,
            };
            write_tensor(
                output,
                &acoustic.run(&features, &controls, style.row(0), speaker, &mut budget)?,
            )?;
            Ok(true)
        }
        [
            cmd,
            model,
            path,
            features,
            context,
            noise,
            shape,
            offset,
            output,
        ] if cmd == "sample-node" => {
            let bytes = read(model, dnni::MAX_BYTES)?;
            let root = dnni::parse(&bytes)?;
            let sampler = roicepeak::sampling::Sampler::compile(select(&root, path)?)?;
            let features = Tensor::from_rmat(&read(features, MAX_FILE_BYTES)?)?;
            let context = Tensor::from_rmat(&read(context, MAX_FILE_BYTES)?)?;
            let noise = Tensor::from_rmat(&read(noise, MAX_FILE_BYTES)?)?;
            let shape = shape
                .parse::<f32>()
                .map_err(|_| Error("invalid sampler shape".into()))?;
            let offset = offset
                .parse::<f32>()
                .map_err(|_| Error("invalid sampler offset".into()))?;
            let mut budget = roicepeak::graph::Budget {
                multiply_adds: 20_000_000_000,
                output_elements: 128 * 1024 * 1024,
            };
            write_tensor(
                output,
                &sampler.run(&features, &context, &noise, shape, offset, &mut budget)?,
            )?;
            Ok(true)
        }
        [cmd, model, input, output] if cmd == "embed-words" || cmd == "language" => {
            let bytes = read(model, dnni::MAX_BYTES)?;
            let root = dnni::parse(&bytes)?;
            let lm = LanguageModel::compile(&root)?;
            let text = String::from_utf8(read(input, 1024 * 1024)?)
                .map_err(|_| Error("words must be UTF-8, one per line".into()))?;
            let words = text.lines().map(str::to_owned).collect::<Vec<_>>();
            let mut budget = roicepeak::graph::Budget {
                multiply_adds: 20_000_000_000,
                output_elements: 128 * 1024 * 1024,
            };
            let y = if cmd == "embed-words" {
                lm.encoder.encode(&words, &mut budget)?
            } else {
                lm.run(&words, &mut budget)?
            };
            write_tensor(output, &y)?;
            Ok(true)
        }
        [cmd] if cmd == "--help" || cmd == "help" => {
            println!(
                "ROICEPEAK 0.1.0 — explicit model inference\n\ninspect MODEL\ntokenize DIC INPUT_UTF8.txt NEW_OUTPUT.tsv\nanalyze-japanese DIC INPUT_UTF8.txt NEW_DIRECTORY\nrender-text MODELS.tsv DIC INPUT_UTF8.txt STAGE_OR_ALL NEW_OUTPUT\ninfer MODEL INPUT NEW_OUTPUT\ninfer-conditioned MODEL INPUT CONDITION NEW_OUTPUT\ninfer-node MODEL CHILD_PATH INPUT NEW_OUTPUT\ninfer-node-conditioned MODEL CHILD_PATH INPUT CONDITION NEW_OUTPUT\ntensor MODEL CHILD_PATH NEW_OUTPUT\nnormalize MODEL CHILD_PATH INPUT NEW_OUTPUT forward|forward-clipped|inverse\nembed-words MODEL WORDS.txt NEW_OUTPUT\nlanguage MODEL WORDS.txt NEW_OUTPUT\nencode-phones PE_MODEL TOKENS.tsv NEW_OUTPUT\nphone-refs PE_MODEL TOKENS.tsv NEW_OUTPUT\nspeaker-vector MODEL OWNER_PATH SPEAKER NEW_OUTPUT\nmix-style MODEL WEIGHTS OFFSETS PHONEMES_OR_DASH NEW_OUTPUT\nrender-phones MODELS.tsv TOKENS.tsv STAGE_OR_ALL NEW_OUTPUT\nprosody IM ACC LINGUISTIC LM WORD_INDEX CODES_OR_DASH IM_SPEAKER ACC_SPEAKER STYLE FORMAT NEW_OUTPUT\nlegacy-acoustic MODEL FEATURES CONTROLS STYLE SPEAKER NEW_OUTPUT\nmodern-pitch MODEL LINGUISTIC CONTROLS STYLE SPEAKER NOISE_OR_DASH NEW_OUTPUT\nmodern-acoustic MODEL LINGUISTIC CONTROLS PITCH STYLE SPEAKER NOISE_OR_DASH NEW_OUTPUT\nsample-node MODEL CHILD_PATH FEATURES CONTEXT NOISE SHAPE OFFSET NEW_OUTPUT\nframe-noise PLAN.tsv CHANNELS SCALE fixed|stream NEW_OUTPUT\nlf-source PHYSICAL SHAPE GAIN NEW_OUTPUT\nvocoder-stage MODEL PHYSICAL STAGE NEW_OUTPUT\nwave INPUT SAMPLE_RATE f32|s16|s24 NEW_OUTPUT.wav\ncompare REFERENCE ACTUAL ABS_TOL REL_TOL\n\nTensors use RMAT. Outputs must not exist. Plain DNNI v1/v2 only.\nNo automatic voice discovery or decryption. Japanese text/native parity remains unqualified."
            );
            Ok(true)
        }
        [cmd, model] if cmd == "inspect" => {
            let bytes = read(model, dnni::MAX_BYTES)?;
            let root = dnni::parse(&bytes)?;
            let mut lines = Vec::new();
            root.inventory(0, &mut lines);
            println!(
                "plain_dnni_version={} nodes={} bytes={}",
                u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
                lines.len(),
                bytes.len()
            );
            for line in lines.iter().take(100) {
                println!("{line}");
            }
            if lines.len() > 100 {
                println!("inventory display truncated at 100 nodes");
            }
            match Graph::compile(&root) {
                Ok(_) => println!("supported_graph=true"),
                Err(e) => println!("supported_graph=false reason={e}"),
            };
            Ok(true)
        }
        [cmd, model, input, output] if cmd == "infer" => infer(model, "root", input, None, output),
        [cmd, model, input, condition, output] if cmd == "infer-conditioned" => {
            infer(model, "root", input, Some(condition), output)
        }
        [cmd, model, path, input, output] if cmd == "infer-node" => {
            infer(model, path, input, None, output)
        }
        [cmd, model, path, input, condition, output] if cmd == "infer-node-conditioned" => {
            infer(model, path, input, Some(condition), output)
        }
        [cmd, model, path, output] if cmd == "tensor" => {
            let bytes = read(model, dnni::MAX_BYTES)?;
            let root = dnni::parse(&bytes)?;
            write_tensor(output, &select(&root, path)?.tensor()?)?;
            Ok(true)
        }
        [cmd, model, path, input, output, direction] if cmd == "normalize" => {
            let bytes = read(model, dnni::MAX_BYTES)?;
            let root = dnni::parse(&bytes)?;
            let normalizer = Normalizer::from_node(select(&root, path)?)?;
            let x = Tensor::from_rmat(&read(input, MAX_FILE_BYTES)?)?;
            let y = match direction.as_str() {
                "forward" => normalizer.normalize(&x, false)?,
                "forward-clipped" => normalizer.normalize(&x, true)?,
                "inverse" => normalizer.denormalize(&x)?,
                _ => return Err(Error("normalizer direction".into())),
            };
            write_tensor(output, &y)?;
            Ok(true)
        }
        [cmd, reference, actual, absolute, relative] if cmd == "compare" => {
            let a = Tensor::from_rmat(&read(reference, MAX_FILE_BYTES)?)?;
            let b = Tensor::from_rmat(&read(actual, MAX_FILE_BYTES)?)?;
            let parse = |s: &str| {
                s.parse::<f64>()
                    .map_err(|_| Error("invalid tolerance".into()))
            };
            let c = compare(&a, &b, parse(absolute)?, parse(relative)?)?;
            if c.elements == 0 {
                return Err(Error(
                    "empty tensors provide no numerical comparison".into(),
                ));
            }
            println!(
                "{{\"elements\":{},\"differing_words\":{},\"failures\":{},\"max_absolute\":{},\"max_relative\":{},\"rms\":{},\"max_ulp\":{},\"worst_index\":{},\"passed\":{}}}",
                c.elements,
                c.differing_words,
                c.failures,
                c.max_absolute,
                c.max_relative,
                c.rms,
                c.max_ulp,
                c.worst_index,
                c.failures == 0
            );
            Ok(c.failures == 0)
        }
        _ => Err(Error("usage: roicepeak-synth --help".into())),
    }
}
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match execute(&args) {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(e) => {
            eprintln!("roicepeak: {e}");
            std::process::exit(2);
        }
    }
}
