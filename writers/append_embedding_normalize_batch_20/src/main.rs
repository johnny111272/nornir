use schemas_embedded::EMBEDDING_TARGET;
use write_core::{OutputFormat, OutputPath, WriteFrequency, WriterConfig};

fn main() {
    write_core::run(&WriterConfig {
        name: "append_embedding_normalize_batch_20",
        schema: &EMBEDDING_TARGET,
        schema_source_path: "/Users/johnny/.ai/spaces/bragi/definitions/schemas/embedding-target.schema.json",
        format: OutputFormat::Jsonl,
        frequency: WriteFrequency::Batch,
        output: OutputPath::FixedFile(
            "/Users/johnny/.ai/spaces/bragi/interview/embedding_format/normalized.jsonl",
        ),
        batch_size: Some(20),
    });
}
