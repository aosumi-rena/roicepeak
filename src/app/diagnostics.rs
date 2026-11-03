//! Headless resource inspection and speech export use the editor's actual catalog/worker.
#[path = "diagnostic_dictionary.rs"]
mod dictionary;
#[path = "diagnostic_export.rs"]
mod export;
#[path = "diagnostic_fft.rs"]
mod fft;
#[path = "diagnostic_graph.rs"]
mod graph;
#[path = "diagnostic_lf_analytic.rs"]
mod lf_analytic;
#[path = "diagnostic_lf_coefficients.rs"]
mod lf_coefficients;
#[path = "diagnostic_lf_shape.rs"]
mod lf_shape;
#[path = "diagnostic_lf_transform.rs"]
mod lf_transform;
#[path = "diagnostic_ltv.rs"]
mod ltv;
#[path = "diagnostic_native_editable.rs"]
mod native_editable;
#[path = "diagnostic_noise.rs"]
mod noise;
#[path = "diagnostic_normalizer.rs"]
mod normalizer;
#[path = "diagnostic_playback.rs"]
mod playback;
#[path = "diagnostic_pqmf.rs"]
mod pqmf;
#[path = "diagnostic_preparation.rs"]
mod preparation;
#[path = "diagnostic_reading.rs"]
mod reading;
#[path = "diagnostic_response.rs"]
mod response;
#[path = "diagnostic_spectra.rs"]
mod spectra;
#[path = "diagnostic_ssml.rs"]
mod ssml;
#[path = "diagnostic_ssml_project.rs"]
mod ssml_project;
#[path = "diagnostic_stages.rs"]
mod stages;
#[path = "diagnostic_text_clipboard.rs"]
mod text_clipboard;
#[path = "diagnostic_vocoder.rs"]
mod vocoder;
#[path = "diagnostic_vocoder_log.rs"]
mod vocoder_log;
use crate::app::{
    catalog::Catalog,
    document::Project,
    render::{Config, Request, Worker},
    resources::Locations,
};
use serde_json::json;
use std::{
    ffi::OsString,
    io::{Read, Write},
    path::Path,
    time::{Duration, Instant},
};

pub fn dispatch(args: &[OsString]) -> Option<i32> {
    let command = args.first()?.to_str()?;
    if ![
        "--inspect-resources",
        "--inspect-graph",
        "--inspect-user-dictionary",
        "--inspect-native-editable",
        "--inspect-portrait",
        "--render-stages",
        "--render-text",
        "--render-project",
        "--process-spectra",
        "--process-response-exponentials",
        "--render-playback",
        "--prepare-project",
        "--inspect-reading",
        "--inspect-ssml-reading",
        "--inspect-ssml",
        "--inspect-ssml-annotations",
        "--export-project",
        "--inspect-postfilter-noise",
        "--inspect-srt",
        "--inspect-ltv-boundaries",
        "--inspect-real-fft",
        "--inspect-normalizer",
        "--inspect-response-endpoint",
        "--inspect-cepstral-response",
        "--inspect-response-phase",
        "--inspect-pqmf",
        "--inspect-lf-shape",
        "--inspect-lf-window",
        "--inspect-lf-coefficients",
        "--inspect-lf-transform",
        "--inspect-lf-analytic",
        "--inspect-vocoder-logarithm",
        "--inspect-vocoder-stages",
        "--inspect-text-clipboard",
    ]
    .contains(&command)
    {
        return None;
    }
    let result = run(command, &args[1..]);
    Some(match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("ROICEPEAK: {error}");
            1
        }
    })
}

fn run(command: &str, args: &[OsString]) -> Result<(), String> {
    if command == "--inspect-graph" {
        return graph::run(args);
    }
    if command == "--inspect-native-editable" {
        return native_editable::run(args);
    }
    if command == "--inspect-user-dictionary" {
        return dictionary::run(args);
    }
    if command == "--inspect-normalizer" {
        return normalizer::run(args);
    }
    if command == "--inspect-ssml" {
        return ssml_project::parse(args);
    }
    if command == "--inspect-ssml-annotations" {
        return ssml_project::annotations(args);
    }
    if command == "--inspect-ssml-reading" {
        return ssml::run(args);
    }
    if command == "--inspect-vocoder-stages" {
        return vocoder::run(args);
    }
    if command == "--inspect-vocoder-logarithm" {
        return vocoder_log::run(args);
    }
    if command == "--inspect-lf-analytic" {
        return lf_analytic::run(args);
    }
    if command == "--inspect-lf-transform" {
        return lf_transform::run(args);
    }
    if command == "--inspect-lf-coefficients" {
        return lf_coefficients::run(args);
    }
    if command == "--inspect-lf-shape" {
        return lf_shape::run(args);
    }
    if command == "--inspect-lf-window" {
        return lf_shape::window(args);
    }
    if command == "--inspect-text-clipboard" {
        return text_clipboard::run(args);
    }
    if command == "--inspect-pqmf" {
        return pqmf::run(args);
    }
    if command == "--inspect-response-phase" {
        return response::phase(args);
    }
    if command == "--inspect-response-endpoint" || command == "--inspect-cepstral-response" {
        return response::run(args, command == "--inspect-response-endpoint");
    }
    if command == "--inspect-real-fft" {
        return fft::run(args);
    }
    if command == "--inspect-ltv-boundaries" {
        return ltv::run(args);
    }
    if command == "--inspect-srt" {
        if args.len() != 1 {
            return Err("Usage: roicepeak --inspect-srt INPUT.srt".into());
        }
        let captions = crate::app::imports::read_srt(Path::new(&args[0]))?;
        serde_json::to_writer_pretty(std::io::stdout().lock(), &captions)
            .map_err(|error| error.to_string())?;
        println!();
        return Ok(());
    }
    if command == "--inspect-postfilter-noise" {
        return noise::run(args);
    }
    if command == "--inspect-reading" {
        return reading::run(args);
    }
    if command == "--export-project" {
        return export::run(args);
    }
    if command == "--process-spectra" || command == "--process-response-exponentials" {
        return spectra::run(args, command == "--process-response-exponentials");
    }
    let (locations, args) = if args
        .first()
        .is_some_and(|argument| argument == "--locations")
    {
        let path = args
            .get(1)
            .ok_or("--locations requires an explicit configuration file")?;
        (Locations::read(Path::new(path))?, &args[2..])
    } else {
        (Locations::load()?, args)
    };
    let (dictionary_path, args) = if args.first().is_some_and(|arg| arg == "--user-dictionary") {
        (
            Some(args.get(1).ok_or("--user-dictionary requires a file")?),
            &args[2..],
        )
    } else {
        (None, args)
    };
    if dictionary_path.is_some()
        && !matches!(
            command,
            "--render-text" | "--render-project" | "--render-playback" | "--prepare-project"
        )
    {
        return Err("--user-dictionary is supported for text/project synthesis only".into());
    }
    let prepare_first = args
        .first()
        .is_some_and(|argument| argument == "--prepare-first");
    let args = if prepare_first { &args[1..] } else { args };
    if (command == "--inspect-resources" && (!args.is_empty() || prepare_first))
        || (command == "--inspect-portrait" && (args.len() != 2 || prepare_first))
        || (command == "--render-stages" && (args.len() != 3 || prepare_first))
        || (command == "--render-text" && args.len() != 3)
        || (command == "--render-project" && args.len() != 2)
        || (command == "--render-playback" && (args.len() != 2 || prepare_first))
        || (command == "--prepare-project" && (args.len() != 2 || prepare_first))
    {
        return Err("Usage: roicepeak --inspect-resources [--locations FILE] | --inspect-portrait [--locations FILE] VOICE_KEY EMOTION_KEY | --render-stages [--locations FILE] VOICE_KEY TOKENS.tsv NEW_OUTPUT_PREFIX | --render-text [--locations FILE] [--prepare-first] VOICE_KEY INPUT_UTF8.txt NEW_OUTPUT.wav | --render-project [--locations FILE] [--user-dictionary FILE] [--prepare-first] INPUT.roicepeak NEW_OUTPUT.wav".into());
    }
    let catalog = Catalog::load_roots(&locations.voice_roots)?;
    if command == "--render-stages" {
        return stages::render(&catalog, args);
    }
    if command == "--inspect-portrait" {
        let key = args[0].to_str().ok_or("Narrator identity must be UTF-8")?;
        let emotion = args[1].to_str().ok_or("Emotion identity must be UTF-8")?;
        let voice = catalog
            .instance(key)
            .ok_or("Requested narrator is not in the resource catalog")?;
        if voice.binding.main_emotion.as_deref() != Some(emotion)
            && !voice
                .binding
                .emotion_bindings
                .iter()
                .any(|control| control.key == emotion)
        {
            return Err("Requested emotion is not declared by this narrator".into());
        }
        let portrait = voice.portrait_sources.load(emotion)?;
        let report = json!({"key":key,"emotion":emotion,
            "portrait":portrait.map(|image| {
                use sha2::{Digest, Sha256};
                json!({"bytes":image.bytes.len(),"pixel_size":image.pixel_size,
                    "default_region":image.default_region,
                    "sha256":crate::app::hex::encode(&Sha256::digest(image.bytes.as_ref()))})
            }),"native_rendering_parity_qualified":false});
        serde_json::to_writer_pretty(std::io::stdout().lock(), &report)
            .map_err(|error| error.to_string())?;
        println!();
        return Ok(());
    }
    if command == "--inspect-resources" {
        let ordered = crate::app::narrator_order::indices(&catalog)?;
        let report = json!({
            "voices": catalog.voices.iter().map(|voice| json!({"key":voice.key,"name":voice.name,
                "version":voice.version,"vendor":voice.vendor,"languages":voice.languages,
                "collection_name":voice.collection_name,
                "can_commercial":voice.can_commercial,"main_emotion":voice.binding.main_emotion,
                "avatar_bytes":voice.avatar.as_ref().map_or(0, Vec::len),
                "portrait":voice.portrait_main.as_ref().map(|portrait| json!({"bytes":portrait.bytes.len(),"pixel_size":portrait.pixel_size,"default_region":portrait.default_region})),
                "emotions":voice.binding.emotion_bindings.iter().map(|control| json!({"label":control.label,"key":control.key})).collect::<Vec<_>>()
            })).collect::<Vec<_>>(),
            "problems":catalog.problems,"dictionary":locations.dictionary,
            "native_ui_icon_directory":crate::app::native_icons::directory(),
            "locales":crate::app::i18n::coverage(),
            "picker_order":ordered.into_iter().map(|index| catalog.voices[index].key.clone()).collect::<Vec<_>>(),
        });
        serde_json::to_writer_pretty(std::io::stdout().lock(), &report)
            .map_err(|e| e.to_string())?;
        println!();
        return Ok(());
    }
    let value_policy = locations.value_policy();
    let dictionary = locations
        .dictionary
        .ok_or("No pronunciation dictionary is available")?;
    let mut config = Config::from_catalog(dictionary, &catalog)?;
    config.value_policy = value_policy;
    let user_dictionary = match dictionary_path {
        Some(path) => crate::app::dictionary::Snapshot::read(Path::new(path))?,
        None => crate::app::dictionary::Snapshot::empty(),
    };
    let (mut project, output_path) = if matches!(
        command,
        "--render-project" | "--render-playback" | "--prepare-project"
    ) {
        (Project::read(Path::new(&args[0]))?, Path::new(&args[1]))
    } else {
        let key = args[0].to_str().ok_or("Narrator identity must be UTF-8")?;
        let voice = catalog
            .instance(key)
            .ok_or("Requested narrator is not in the resource catalog")?;
        let mut text = String::new();
        std::fs::File::open(Path::new(&args[1]))
            .map_err(|e| e.to_string())?
            .take(1_000_001)
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        if text.len() > 1_000_000 {
            return Err("Speech input exceeds 1 MiB".into());
        }
        let mut project = Project::default();
        project.blocks[0].narrator = crate::app::document::VoiceRef {
            identity: voice.name.clone(),
            language: "japanese".into(),
            version: crate::app::document::VoiceVersion::Exact(voice.version),
        };
        project.blocks[0].set_text(text)?;
        (project, Path::new(&args[2]))
    };
    if output_path.exists() {
        return Err("Output already exists".into());
    }
    if command == "--render-playback" {
        return playback::render(config, project, user_dictionary, output_path);
    }
    if command == "--prepare-project" {
        return preparation::prepare(config, project, user_dictionary, output_path);
    }
    let narrators: std::collections::BTreeSet<_> =
        project.blocks.iter().map(|b| b.narrator.clone()).collect();
    let mut worker = Worker::default();
    let preparation = if prepare_first {
        worker.prepare(Request {
            config: config.clone(),
            project: project.clone(),
            user_dictionary: user_dictionary.clone(),
        })?;
        let prepared = wait(|| worker.poll_prepare())?;
        project = crate::app::preparation_ui::apply(&project, &user_dictionary, &prepared)?;
        Some(prepared.profile)
    } else {
        None
    };
    worker.submit(Request {
        config,
        project,
        user_dictionary,
    })?;
    let started = Instant::now();
    let rendered = wait(|| worker.poll())?;
    let samples =
        roicepeak::tensor::Tensor::new(rendered.pcm.len(), 1, rendered.pcm.as_ref().clone())
            .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    roicepeak::wave::write(
        &mut bytes,
        &samples,
        rendered.rate,
        roicepeak::wave::Encoding::Float32,
    )
    .map_err(|e| e.to_string())?;
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)
        .map_err(|e| e.to_string())?;
    output.write_all(&bytes).map_err(|e| e.to_string())?;
    println!(
        "{}",
        json!({"narrators":narrators,"samples":rendered.pcm.len(),"sample_rate":rendered.rate,
            "blocks":rendered.blocks.iter().map(|block| json!({"id":block.id,"start":block.start,"duration":block.duration,"segments":block.segments.len()})).collect::<Vec<_>>(),
            "elapsed_seconds":started.elapsed().as_secs_f64(),"profile":rendered.profile,"preparation_profile":preparation,
            "diagnostics_path":crate::app::render::diagnostics_path(),"native_parity_qualified":false})
    );
    Ok(())
}

fn wait<T>(mut poll: impl FnMut() -> Option<Result<T, String>>) -> Result<T, String> {
    let started = Instant::now();
    loop {
        if let Some(result) = poll() {
            return result;
        }
        if started.elapsed() > Duration::from_secs(1800) {
            return Err("Speech generation exceeded 30 minutes".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
