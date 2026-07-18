use std::{io::SeekFrom, path::Path, process::Stdio};

use tempfile::TempDir;
use thiserror::Error;
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt},
    process::Command,
};

const MAX_EXTRACTED_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum DocumentError {
    #[error("document filename has no supported extension")]
    Unsupported,
    #[error("failed to prepare document: {0}")]
    Io(#[from] std::io::Error),
    #[error("{program} failed: {message}")]
    Extractor { program: String, message: String },
    #[error("document contains no extractable text")]
    Empty,
    #[error("extracted document is too large")]
    TooLarge,
}

pub async fn extract(bytes: &[u8], filename: &str) -> Result<String, DocumentError> {
    let extension = supported_extension(filename).ok_or(DocumentError::Unsupported)?;
    let directory = TempDir::new()?;
    let input = directory.path().join(format!("input.{extension}"));
    fs::write(&input, bytes).await?;

    let output = match extension {
        "pdf" => run("pdftotext", &["-layout", path(&input), "-"]).await?,
        "doc" => run("antiword", &[path(&input)]).await?,
        "docx" => convert_with_libreoffice(directory.path(), &input, "txt:Text").await?,
        "xls" | "xlsx" => {
            convert_with_libreoffice(directory.path(), &input, "csv:Text - txt - csv (StarCalc)")
                .await?
        }
        _ => return Err(DocumentError::Unsupported),
    };
    if output.len() > MAX_EXTRACTED_BYTES {
        return Err(DocumentError::TooLarge);
    }
    let normalized = crate::text::normalize(&output);
    if normalized.is_empty() {
        Err(DocumentError::Empty)
    } else {
        Ok(normalized)
    }
}

pub fn supported_extension(filename: &str) -> Option<&str> {
    let extension = filename.rsplit_once('.')?.1;
    ["doc", "docx", "pdf", "xls", "xlsx"]
        .iter()
        .find(|candidate| extension.eq_ignore_ascii_case(candidate))
        .copied()
}

async fn convert_with_libreoffice(
    directory: &Path,
    input: &Path,
    format: &str,
) -> Result<String, DocumentError> {
    let output = Command::new("libreoffice")
        .args([
            "--headless",
            "--nologo",
            "--nodefault",
            "--nolockcheck",
            "--nofirststartwizard",
            "--convert-to",
            format,
            "--outdir",
            path(directory),
            path(input),
        ])
        .stdin(Stdio::null())
        .output()
        .await?;
    if !output.status.success() {
        return Err(DocumentError::Extractor {
            program: "libreoffice".into(),
            message: String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(1000)
                .collect(),
        });
    }
    let extension = if format.starts_with("csv") {
        "csv"
    } else {
        "txt"
    };
    let output_path = directory.join(format!("input.{extension}"));
    read_path_capped(&output_path, MAX_EXTRACTED_BYTES).await
}

async fn run(program: &str, arguments: &[&str]) -> Result<String, DocumentError> {
    let stdout = tempfile::tempfile()?;
    let stderr = tempfile::tempfile()?;
    let status = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout.try_clone()?))
        .stderr(Stdio::from(stderr.try_clone()?))
        .status()
        .await?;
    if !status.success() {
        return Err(DocumentError::Extractor {
            program: program.into(),
            message: read_file_prefix(stderr, 1000).await?,
        });
    }
    read_file_capped(stdout, MAX_EXTRACTED_BYTES).await
}

async fn read_path_capped(path: &Path, maximum: usize) -> Result<String, DocumentError> {
    if fs::metadata(path).await?.len() > maximum as u64 {
        return Err(DocumentError::TooLarge);
    }
    let bytes = fs::read(path).await?;
    if bytes.len() > maximum {
        return Err(DocumentError::TooLarge);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

async fn read_file_capped(file: std::fs::File, maximum: usize) -> Result<String, DocumentError> {
    if file.metadata()?.len() > maximum as u64 {
        return Err(DocumentError::TooLarge);
    }
    let mut file = fs::File::from_std(file);
    file.seek(SeekFrom::Start(0)).await?;
    let mut bytes = Vec::with_capacity(maximum.min(64 * 1024));
    file.take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > maximum {
        return Err(DocumentError::TooLarge);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

async fn read_file_prefix(file: std::fs::File, maximum: usize) -> Result<String, DocumentError> {
    let mut file = fs::File::from_std(file);
    file.seek(SeekFrom::Start(0)).await?;
    let mut bytes = Vec::with_capacity(maximum);
    file.take(maximum as u64).read_to_end(&mut bytes).await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn path(value: &Path) -> &str {
    value.to_str().expect("temporary paths must be valid UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_supported_extensions_case_insensitively() {
        assert_eq!(supported_extension("Informe.PDF"), Some("pdf"));
        assert_eq!(supported_extension("datos.xlsx"), Some("xlsx"));
        assert_eq!(supported_extension("archivo"), None);
        assert_eq!(supported_extension("malware.exe"), None);
    }

    #[tokio::test]
    async fn rejects_unsupported_before_using_external_programs() {
        assert!(matches!(
            extract(b"data", "file.exe").await,
            Err(DocumentError::Unsupported)
        ));
    }

    #[tokio::test]
    async fn wraps_extractor_failures_without_panicking() {
        let error = run("false", &[]).await.unwrap_err();
        assert!(matches!(
            error,
            DocumentError::Extractor { ref program, .. } if program == "false"
        ));

        let output = run("printf", &["  hola  "]).await.unwrap();
        assert_eq!(output, "  hola  ");
    }

    #[tokio::test]
    async fn rejects_extractor_output_before_loading_it_into_memory() {
        let file = tempfile::tempfile().unwrap();
        file.set_len(MAX_EXTRACTED_BYTES as u64 + 1).unwrap();

        assert!(matches!(
            read_file_capped(file, MAX_EXTRACTED_BYTES).await,
            Err(DocumentError::TooLarge)
        ));
    }

    #[tokio::test]
    async fn invalid_pdf_is_rejected_and_temporary_files_are_cleaned() {
        let result = extract(b"this is not a PDF", "archivo.pdf").await;
        assert!(
            matches!(
                result,
                Err(DocumentError::Extractor { ref program, .. }) if program == "pdftotext"
            ) || matches!(
                result,
                Err(DocumentError::Io(ref error))
                    if error.kind() == std::io::ErrorKind::NotFound
            )
        );
    }
}
