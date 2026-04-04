pub fn mean_pool(last_hidden: &[f32], attention_mask: &[i64], dims: usize) -> Vec<f32> {
    let seq_len = last_hidden.len() / dims;
    let mut pooled = vec![0.0f32; dims];
    let mut mask_sum = 0.0f32;

    for i in 0..seq_len {
        let mask_val = if attention_mask[i] == 1 { 1.0f32 } else { 0.0f32 };
        mask_sum += mask_val;
        for j in 0..dims {
            pooled[j] += mask_val * last_hidden[i * dims + j];
        }
    }

    if mask_sum > 0.0 {
        for v in pooled.iter_mut() {
            *v /= mask_sum;
        }
    }

    pooled
}

#[cfg(test)]
mod tests {
    use super::*;
    use ort::session::Session;
    use ort::value::Value;
    use std::path::PathBuf;
    use std::time::Instant;

    fn model_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap();
        PathBuf::from(home).join(".sqmd").join("models").join("nomic-embed-text-v1.5-q8.onnx")
    }

    #[test]
    fn test_ort_load_and_embed() {
        let path = model_path();
        if !path.exists() {
            println!("Skipping: model not found at {:?}", path);
            return;
        }

        let start = Instant::now();
        let session = Session::builder()
            .unwrap()
            .commit_from_file(&path)
            .unwrap();
        let load_time = start.elapsed();
        println!("Model load time: {:?}", load_time);
        assert!(load_time.as_secs() < 10, "Model loading took too long: {:?}", load_time);

        let input_names: Vec<String> = session.inputs().iter().map(|i| i.name().to_string()).collect();
        let output_names: Vec<String> = session.outputs().iter().map(|o| o.name().to_string()).collect();
        println!("Inputs: {:?}", input_names);
        println!("Outputs: {:?}", output_names);

        let seq_len = 64;
        let dummy_tokens: Vec<i64> = (0..seq_len).map(|i| if i < 5 { (i as i64) + 101 } else { 0 }).collect();
        let attention_mask: Vec<i64> = (0..seq_len).map(|i| if i < 5 { 1 } else { 0 }).collect();

        let input_ids_val = Value::from_array(([1usize, seq_len], dummy_tokens.clone())).unwrap();
        let attention_val = Value::from_array(([1usize, seq_len], attention_mask.clone())).unwrap();
        let token_types_val = Value::from_array(([1usize, seq_len], vec![0i64; seq_len])).unwrap();

        let inputs: Vec<(&str, ort::value::Value)> = vec![
            (input_names[0].as_str(), input_ids_val.into()),
            (input_names[1].as_str(), attention_val.into()),
            (input_names[2].as_str(), token_types_val.into()),
        ];

        let mut session = session;
        let infer_start = Instant::now();
        let outputs = session.run(inputs).unwrap();
        let infer_time = infer_start.elapsed();
        println!("Inference time: {:?}", infer_time);
        assert!(infer_time.as_millis() < 2000, "Inference too slow: {:?}", infer_time);

        let (shape, data) = outputs[0].try_extract_tensor::<f32>().unwrap();
        println!("Output shape: {:?}", shape);
        println!("Output len: {}", data.len());

        let pooled = mean_pool(data, &attention_mask, 768);
        let norm: f32 = pooled.iter().map(|v| v * v).sum::<f32>().sqrt();
        println!("Embedding norm: {}", norm);
        assert!(norm > 0.0, "Embedding is all zeros");
        assert_eq!(pooled.len(), 768);
    }
}
