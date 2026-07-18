pub fn normalize(input: &str) -> String {
    input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn chunks(input: &str, maximum_chars: usize, overlap_chars: usize) -> Vec<String> {
    assert!(maximum_chars > 0);
    assert!(overlap_chars < maximum_chars);

    let normalized = normalize(input);
    let chars: Vec<char> = normalized.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let mut end = (start + maximum_chars).min(chars.len());
        if end < chars.len()
            && let Some(boundary) = chars[start..end]
                .iter()
                .rposition(|character| character.is_whitespace())
            && boundary > maximum_chars / 2
        {
            end = start + boundary;
        }
        let chunk: String = chars[start..end].iter().collect();
        result.push(chunk.trim().to_owned());
        if end == chars.len() {
            break;
        }
        start = end.saturating_sub(overlap_chars);
    }
    result
}

pub fn source_context(sources: &[crate::repository::SearchHit]) -> String {
    sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            format!(
                "[{}] mensaje={} fecha={} autor={}\n{}",
                index + 1,
                source.whatsapp_message_id,
                source
                    .source_timestamp
                    .map(|date| date.to_rfc3339())
                    .unwrap_or_else(|| "desconocida".into()),
                source.sender_name.as_deref().unwrap_or("desconocido"),
                source.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    use super::*;

    #[test]
    fn normalizes_blank_lines_and_surrounding_space() {
        assert_eq!(normalize("  uno  \n\n dos \r\n"), "uno\ndos");
    }

    #[test]
    fn chunks_unicode_without_breaking_characters() {
        let result = chunks("áéíóú palabras para dividir correctamente", 20, 4);

        assert!(result.len() >= 2);
        assert!(result.iter().all(|chunk| chunk.chars().count() <= 20));
        assert!(result.join(" ").contains("áéíóú"));
    }

    #[test]
    fn empty_input_has_no_chunks() {
        assert!(chunks(" \n ", 100, 10).is_empty());
    }

    #[test]
    #[should_panic]
    fn rejects_overlap_equal_to_size() {
        chunks("text", 4, 4);
    }

    #[test]
    fn formats_sources_as_numbered_citations() {
        let source = crate::repository::SearchHit {
            chunk_id: Uuid::new_v4(),
            message_id: Uuid::new_v4(),
            whatsapp_message_id: "wamid.1".into(),
            sender_name: Some("Ana".into()),
            source_timestamp: Some(Utc.timestamp_opt(1_700_000_000, 0).unwrap()),
            content: "La reunión es el viernes.".into(),
            score: 0.9,
        };

        let context = source_context(&[source]);

        assert!(context.starts_with("[1] mensaje=wamid.1"));
        assert!(context.contains("autor=Ana"));
        assert!(context.ends_with("La reunión es el viernes."));
        assert!(source_context(&[]).is_empty());
    }
}
