use std::{backtrace::Backtrace, io::Write};

pub(crate) fn install() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        report(&format!("{info}\n{}", Backtrace::force_capture()));
        previous(info);
    }));
}

pub(crate) fn report(message: &str) {
    let Some(directory) = crate::platform::config_dir() else {
        return;
    };
    if std::fs::create_dir_all(&directory).is_err() {
        return;
    }
    let Ok(mut file) = std::fs::File::create(directory.join("last-error.log")) else {
        return;
    };
    let bytes = message.as_bytes();
    let _ = file.write_all(&bytes[..bytes.len().min(65_536)]);
}
