use std::io::Read;

pub struct OllamaClient {
    base_url: String,
    model: String,
}

impl OllamaClient {
    pub fn new() -> Self {
        let base_url =
            std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".to_string());
        let model = std::env::var("SQMD_HINT_MODEL").unwrap_or_else(|_| "gemma3:4b".to_string());
        Self { base_url, model }
    }

    pub fn generate_prospective_hints(
        &self,
        content: &str,
    ) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let truncated = if content.len() > 3000 {
            &content[..3000]
        } else {
            content
        };

        let prompt = format!(
            "You are a search query generator. Given the following code or text, generate exactly 3 short natural-language queries that someone might type into a search engine to find this content. Each query should be on its own line. Do not include numbering, bullets, or explanations. Just the queries.\n\nContent:\n{}",
            truncated
        );

        let body = serde_json::json!({
            "model": self.model,
            "prompt": prompt,
            "stream": false
        });

        let url = format!("{}/api/generate", self.base_url);
        let response = ureq::Agent::new_with_defaults()
            .post(&url)
            .send_json(&body)?;

        let mut body_str = String::new();
        response
            .into_body()
            .into_reader()
            .read_to_string(&mut body_str)?;
        let parsed: serde_json::Value = serde_json::from_str(&body_str)?;

        let response_text = parsed["response"].as_str().unwrap_or("").to_string();

        let hints: Vec<String> = response_text
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .take(3)
            .collect();

        Ok(hints)
    }
}
