use crate::app::{
    document::Project,
    render::{Config, Request, Worker},
};
use serde_json::json;
use std::{io::Write, path::Path};

pub(super) fn prepare(
    config: Config,
    project: Project,
    user_dictionary: crate::app::dictionary::Snapshot,
    output: &Path,
) -> Result<(), String> {
    let mut worker = Worker::default();
    worker.prepare(Request {
        config,
        project: project.clone(),
        user_dictionary: user_dictionary.clone(),
    })?;
    let prepared = super::wait(|| worker.poll_prepare())?;
    let result = crate::app::preparation_ui::apply(&project, &user_dictionary, &prepared)?;
    let data = serde_json::to_vec_pretty(&result).map_err(|error| error.to_string())?;
    if data.len() > 16 * 1024 * 1024 {
        return Err("Prepared project exceeds 16 MiB".into());
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| error.to_string())?;
    file.write_all(&data).map_err(|error| error.to_string())?;
    println!(
        "{}",
        json!({
            "format":result.format, "profile":prepared.profile,
            "blocks":result.blocks.iter().map(|block| json!({
                "id":block.id, "sentences":block.sentences.len(),
                "words":block.sentences.iter().map(|sentence| sentence.words.len()).sum::<usize>(),
                "manual_words":block.sentences.iter().flat_map(|sentence| &sentence.words).filter(|word| word.manual()).count()
            })).collect::<Vec<_>>(), "native_parity_qualified":false
        })
    );
    Ok(())
}
