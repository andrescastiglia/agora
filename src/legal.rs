use axum::{
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};

const PRIVACY: &str = include_str!("../legal/privacy.html");
const TERMS: &str = include_str!("../legal/terms.html");
const DATA_DELETION: &str = include_str!("../legal/data-deletion.html");

fn html(document: &'static str) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (
                header::CACHE_CONTROL,
                "public, max-age=3600, must-revalidate",
            ),
        ],
        document,
    )
        .into_response()
}

pub async fn privacy() -> Response {
    html(PRIVACY)
}

pub async fn terms() -> Response {
    html(TERMS)
}

pub async fn data_deletion() -> Response {
    html(DATA_DELETION)
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;

    use super::*;

    #[tokio::test]
    async fn legal_documents_are_public_html_and_name_the_service() {
        for response in [privacy().await, terms().await, data_deletion().await] {
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers()[header::CONTENT_TYPE],
                "text/html; charset=utf-8"
            );
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            assert!(String::from_utf8(body.to_vec()).unwrap().contains("Agora"));
        }
    }
}
