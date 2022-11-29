use aws_sdk_s3::{types::ByteStream, Client, Region};
use bytes::Bytes;
use concourse_resource::*;
use glob::glob;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;
use tokio::{fs::File, runtime::Builder};
use tokio_stream::StreamExt;

pub struct S3WriteOnly;

#[derive(Deserialize, Debug, Serialize, Clone)]
pub struct Version {
    #[serde(rename = "path")]
    pub path: String,
}

#[derive(Deserialize, Default, Clone)]
pub struct Source {
    pub bucket: String,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub session_token: Option<String>,
    pub aws_role_arn: Option<String>,
    pub region_name: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct InParams {}

#[derive(Deserialize, Debug)]
#[serde(default)]
pub struct OutParams {
    pub glob: String,
    #[serde(with = "serde_regex")]
    pub except_regex: Option<Regex>,
    pub s3_prefix: String,
}
impl Default for OutParams {
    fn default() -> Self {
        OutParams {
            glob: String::from(""),
            except_regex: None,
            s3_prefix: String::from(""),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, IntoMetadataKV, Clone)]
pub struct Metadata {
    pub path: String,
}

impl Resource for S3WriteOnly {
    type Version = Version;

    type Source = Source;

    type InParams = InParams;

    type InMetadata = Metadata;

    type OutParams = OutParams;

    type OutMetadata = Metadata;

    fn resource_check(
        _source: Option<Self::Source>,
        version: Option<Self::Version>,
    ) -> Vec<Self::Version> {
        if let Some(v) = version {
            vec![v]
        } else {
            vec![]
        }
    }

    fn resource_in(
        source: Option<Self::Source>,
        version: Self::Version,
        _params: Option<Self::InParams>,
        output_path: &str,
    ) -> Result<InOutput<Self::Version, Self::InMetadata>, Box<dyn std::error::Error>> {
        let path = version.path.clone();
        let source = source.unwrap();
        let s3_prefix = version.path.clone();
        eprintln!(
            "Downloading files from s3://{}/{}",
            &source.bucket, &s3_prefix
        );

        let runtime = Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let mut handles = Vec::new();

        let region = match source.region_name {
            Some(r) => r,
            None => "us-east-1".into(),
        };
        let paths = runtime.block_on(list_objects(
            source.bucket.clone(),
            region.clone(),
            s3_prefix.clone(),
        ));
        for path in paths.unwrap() {
            let filename = Path::new(&path).file_name().unwrap();
            let file_path = Path::new(output_path)
                .join(filename)
                .to_str()
                .unwrap()
                .replace("//", "/");
            eprintln!("Downloading {path} to {file_path}");

            handles.push(runtime.spawn(download_object(
                source.bucket.clone(),
                region.clone(),
                path,
                file_path,
            )));
        }
        eprintln!("Will await {} downloads", handles.len());

        // Wait for all of them to complete.
        for handle in handles {
            // The `spawn` method returns a `JoinHandle`. A `JoinHandle` is
            // a future, so we can wait for it using `block_on`.
            match runtime.block_on(handle) {
                Ok(_) => (),
                Err(e) => eprintln!("An error occurred: {e}"),
            }
        }

        Ok(InOutput {
            version,
            metadata: Some(Self::InMetadata { path }),
        })
    }

    fn resource_out(
        source: Option<Self::Source>,
        params: Option<Self::OutParams>,
        input_path: &str,
    ) -> OutOutput<Self::Version, Self::OutMetadata> {
        let source = source.unwrap();
        let params = params.unwrap();
        let bd = Self::build_metadata();
        let s3_prefix = params.s3_prefix.replace("{BUILD_ID}", &bd.id);
        let s3_prefix = s3_prefix.replace(
            "{BUILD_NAME}",
            &bd.name.expect("Expected BUILD_NAME env var to be present."),
        );
        let s3_prefix = s3_prefix.replace(
            "{BUILD_JOB_NAME}",
            &bd.job_name
                .expect("Expected BUILD_NAME env var to be present."),
        );
        let s3_prefix = s3_prefix.replace(
            "{BUILD_PIPELINE_NAME}",
            &bd.pipeline_name
                .expect("Expected BUILD_NAME env var to be present."),
        );
        let s3_prefix = s3_prefix.replace("{BUILD_TEAM_NAME}", &bd.team_name);
        eprintln!(
            "Saving files from '{}' to s3://{}/{}",
            &params.glob, &source.bucket, &s3_prefix
        );

        let runtime = Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let mut handles = Vec::new();

        let path_and_pattern = format!("{}/{}", input_path, params.glob);
        let paths = glob(&path_and_pattern).expect("Failed to read glob pattern");
        let region = match source.clone().region_name {
            Some(r) => r,
            None => "us-east-1".into(),
        };
        for entry in paths {
            match entry {
                Ok(path) => {
                    if let Some(ref regex) = params.except_regex {
                        if regex.is_match(path.to_str().unwrap()) {
                            eprintln!("Ignoring: {:?}", path.display());
                        } else {
                            queue_upload(
                                path,
                                &mut handles,
                                &runtime,
                                &source,
                                &region,
                                &s3_prefix,
                            );
                        }
                    } else {
                        queue_upload(path, &mut handles, &runtime, &source, &region, &s3_prefix);
                    }
                }
                Err(e) => eprintln!("{:?}", e),
            }
        }

        // Wait for all of them to complete.
        for handle in handles {
            // The `spawn` method returns a `JoinHandle`. A `JoinHandle` is
            // a future, so we can wait for it using `block_on`.
            match runtime.block_on(handle) {
                Ok(_) => (),
                Err(e) => eprintln!("An error occurred: {e}"),
            }
        }

        OutOutput {
            version: Self::Version { path: s3_prefix },
            metadata: None,
        }
    }

    fn build_metadata() -> BuildMetadata {
        BuildMetadata {
            id: std::env::var("BUILD_ID").expect("environment variable BUILD_ID should be present"),
            name: std::env::var("BUILD_NAME").ok(),
            job_name: std::env::var("BUILD_JOB_NAME").ok(),
            pipeline_name: std::env::var("BUILD_PIPELINE_NAME").ok(),
            pipeline_instance_vars: std::env::var("BUILD_PIPELINE_INSTANCE_VARS")
                .ok()
                .and_then(|instance_vars| serde_json::from_str(&instance_vars[..]).ok()),
            team_name: std::env::var("BUILD_TEAM_NAME")
                .expect("environment variable BUILD_TEAM_NAME should be present"),
            atc_external_url: std::env::var("ATC_EXTERNAL_URL")
                .expect("environment variable ATC_EXTERNAL_URL should be present"),
        }
    }
}

fn queue_upload(
    path: PathBuf,
    handles: &mut Vec<JoinHandle<Result<(), Box<dyn Error + Send + Sync>>>>,
    runtime: &Runtime,
    source: &Source,
    region: &String,
    s3_prefix: &String,
) {
    eprintln!("{:?}", path.display());
    let filename = path.file_name().unwrap();
    let key = format!("{s3_prefix}/{}", filename.to_str().unwrap()).replace("//", "/");
    let file_path = path.into_os_string().into_string().unwrap();
    handles.push(runtime.spawn(upload_object(
        source.bucket.clone(),
        region.clone(),
        file_path,
        key,
    )));
}

async fn upload_object(
    bucket_name: String,
    region: String,
    file_name: String,
    key: String,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = aws_config::from_env()
        .region(Region::new(region))
        .load()
        .await;
    let client = Client::new(&config);

    let body = ByteStream::from_path(Path::new(&file_name)).await;
    let result = client
        .put_object()
        .bucket(bucket_name)
        .key(key)
        .body(body.unwrap())
        .send()
        .await;
    match result {
        Ok(_) => eprintln!("Uploaded file: {}", file_name),
        Err(e) => {
            eprintln!("Error uploading file: {}\nERROR: {}", file_name, e);
            return Err(e.into());
        }
    };
    Ok(())
}

pub async fn list_objects(
    bucket_name: String,
    region: String,
    prefix: String,
) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
    let config = aws_config::from_env()
        .region(Region::new(region))
        .load()
        .await;
    let client = Client::new(&config);
    let objects = client
        .list_objects_v2()
        .bucket(bucket_name)
        .prefix(prefix)
        .send()
        .await?;
    let files: Vec<String> = objects
        .contents()
        .unwrap_or_default()
        .iter()
        .map(|o| o.key().unwrap().into())
        .collect();

    Ok(files)
}

pub async fn download_object(
    bucket_name: String,
    region: String,
    key: String,
    file_name: String,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = aws_config::from_env()
        .region(Region::new(region))
        .load()
        .await;
    let client = Client::new(&config);
    let resp = client
        .get_object()
        .bucket(bucket_name)
        .key(&key)
        .send()
        .await;
    match resp {
        Ok(o) => {
            let mut file = match File::create(&file_name).await {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("Error opening file: {file_name}\n{e}");
                    return Err(Box::new(e));
                }
            };

            let mut stream = o.body;
            while let Some(bytes) = stream.next().await {
                let bytes: Bytes = bytes?;
                file.write_all(&bytes).await?;
            }
            file.flush().await?;
            eprintln!("Downloaded {key} to {file_name}");
            Ok(())
        }
        Err(e) => {
            eprintln!("Error downloading file: {}\nERROR: {}", file_name, e);
            Err(e.into())
        }
    }
}

#[cfg(test)]
mod check_tests;

#[cfg(test)]
mod in_tests;

#[cfg(test)]
mod out_tests;
