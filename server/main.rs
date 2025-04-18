use code_executor::executor_server::{Executor, ExecutorServer};
use code_executor::{CodeReply, CodeRequest};
use std::process::Command;
use std::{env, fs};
use tonic::{transport::Server, Request, Response, Status};

pub mod code_executor {
    tonic::include_proto!("code_executor");
}

#[derive(Default)]
pub struct MyExecutor {}

#[tonic::async_trait]
impl Executor for MyExecutor {
    async fn execute(&self, request: Request<CodeRequest>) -> Result<Response<CodeReply>, Status> {
        let req = request.into_inner();

        println!("📦 입력된 언어: {}", req.language);
        println!("📦 입력된 코드:\n{}", req.source_code);
        println!("📦 입력된 옵션: {}", req.option);

        let ext = match req.language.as_str() {
            "c99" => "c",
            "c++17" | "c++20" => "cc",
            "java8" => "java",
            "python3" | "pypy" => "py",
            _ => {
                return Err(Status::invalid_argument("지원하지 않는 언어입니다."));
            }
        };

        let cur_dir_path = env::current_dir()?
            .join("shared")
            .to_str()
            .unwrap()
            .to_string();

        // shared 디렉토리에 사용자의 소스코드를 Main.<ext>로 저장
        let source_file_name = format!("Main.{}", ext);
        let source_file_path = format!("{}/{}", cur_dir_path, source_file_name);
        fs::write(&source_file_path, &req.source_code)
            .map_err(|e| Status::internal(format!("파일 저장 실패: {}", e)))?;

        // [run, judge] 옵션에 따라 실행
        match req.option.as_str() {
            "run" => {
                println!("📦 소스코드 컴파일...");
                compile_on_docker(req.language.clone(), source_file_name, "Main".to_string())
                    .await
                    .map_err(|e| Status::internal(format!("소스코드 컴파일 실패: {}", e)))?;

                println!("📦 실행...");
                let output = run_on_docker(req.language.clone(), "Main".to_string(), None, None)
                    .await
                    .map_err(|e| Status::internal(format!("실행 실패: {}", e)))?;

                println!("📦 실행 결과:\n{}", output);

                Ok(Response::new(CodeReply { result: output }))
            }
            "judge" => {
                println!("📦 소스코드 컴파일...");
                compile_on_docker(req.language.clone(), source_file_name, "Main".to_string())
                    .await
                    .map_err(|e| Status::internal(format!("소스코드 컴파일 실패: {}", e)))?;

                println!("📦 정해 컴파일...");
                compile_on_docker(
                    "c++17".to_string(),
                    "solution.cpp".to_string(),
                    "solution".to_string(),
                )
                .await
                .map_err(|e| Status::internal(format!("정해 컴파일 실패: {}", e)))?;

                println!("📦 제너레이터 컴파일...");
                compile_on_docker(
                    "c++17".to_string(),
                    "testlib/generator.cpp".to_string(),
                    "testlib/generator".to_string(),
                )
                .await
                .map_err(|e| Status::internal(format!("제너레이터 컴파일 실패: {}", e)))?;

                let num_test_cases = 3;
                println!("📦 데이터 생성...");
                for i in 0..num_test_cases {
                    let input_file = format!("input/{}.in", i);

                    Command::new("docker")
                        .args([
                            "run",
                            "--rm",
                            "-v",
                            &format!("{}:/app/shared", cur_dir_path),
                            "worker",
                            "/app/generator.sh",
                            "--EXECUTABLE=testlib/generator",
                            format!("--RANDOM_SEED={}", i).as_str(),
                            format!("--INPUT_FILE={}", input_file).as_str(),
                        ])
                        .output()
                        .map_err(|e| Status::internal(format!("데이터 생성 실패: {}", e)))?;
                }

                println!("📦 체커 컴파일...");
                compile_on_docker(
                    "c++17".to_string(),
                    "testlib/checker.cpp".to_string(),
                    "testlib/checker".to_string(),
                )
                .await
                .map_err(|e| Status::internal(format!("체커 컴파일 실패: {}", e)))?;

                println!("📦 채점...");
                let mut result = String::new();
                for i in 0..num_test_cases {
                    let input_file = format!("input/{}.in", i);
                    let output_file = format!("output/{}.out", i);
                    let answer_file = format!("answer/{}.out", i);

                    run_on_docker(
                        req.language.clone(),
                        "Main".to_string(),
                        Some(input_file.clone()),
                        Some(output_file.clone()),
                    )
                    .await
                    .map_err(|e| Status::internal(format!("소스코드 실행 실패: {}", e)))?;

                    run_on_docker(
                        "c++17".to_string(),
                        "solution".to_string(),
                        Some(input_file.clone()),
                        Some(answer_file.clone()),
                    )
                    .await
                    .map_err(|e| Status::internal(format!("정해 실행 실패: {}", e)))?;

                    let output = Command::new("docker")
                        .args([
                            "run",
                            "--rm",
                            "-v",
                            &format!("{}:/app/shared", cur_dir_path),
                            "worker",
                            "/app/checker.sh",
                            "--EXECUTABLE=testlib/checker",
                            format!("--INPUT_FILE={}", input_file).as_str(),
                            format!("--OUTPUT_FILE={}", output_file).as_str(),
                            format!("--ANSWER_FILE={}", answer_file).as_str(),
                        ])
                        .output()
                        .map_err(|e| Status::internal(format!("체커 실행 실패: {}", e)))?;

                    result.push_str(&format!(
                        "테스트 케이스 {}: {}\n",
                        i,
                        String::from_utf8_lossy(&output.stderr).to_string()
                    ));
                }

                Ok(Response::new(CodeReply {
                    result: format!("채점 완료! 결과:\n{}", result),
                }))
            }
            _ => Err(Status::invalid_argument("지원하지 않는 옵션입니다.")),
        }
    }
}

async fn compile_on_docker(
    language: String,
    source_file: String,
    executable_file: String,
) -> Result<String, Status> {
    let cur_dir_path = env::current_dir()?
        .join("shared")
        .to_str()
        .unwrap()
        .to_string();
    let volume_arg = format!("{}:/app/shared", cur_dir_path);
    let language_arg = format!("--LANGUAGE={}", language);
    let source_arg = format!("--SOURCE_FILE={}", source_file);
    let executable_arg = format!("--EXECUTABLE={}", executable_file);

    let args = vec![
        "run",
        "--rm",
        "-v",
        &volume_arg,
        "worker",
        "/app/compile.sh",
        &language_arg,
        &source_arg,
        &executable_arg,
    ];

    let output = Command::new("docker")
        .args(args)
        .output()
        .map_err(|e| Status::internal(format!("Docker 실행 실패: {}", e)))?;

    if !output.status.success() {
        return Err(Status::internal(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

async fn run_on_docker(
    language: String,
    executable: String,
    input_file: Option<String>,
    output_file: Option<String>,
) -> Result<String, Status> {
    let cur_dir_path = env::current_dir()?
        .join("shared")
        .to_str()
        .unwrap()
        .to_string();
    let volume_arg = format!("{}:/app/shared", cur_dir_path);
    let language_arg = format!("--LANGUAGE={}", language);
    let executable_arg = format!("--EXECUTABLE={}", executable);
    let input_arg = input_file
        .map(|file| format!("--INPUT_FILE={}", file))
        .unwrap_or_default();
    let output_arg = output_file
        .map(|file| format!("--OUTPUT_FILE={}", file))
        .unwrap_or_default();

    let mut args = vec![
        "run",
        "--rm",
        "-v",
        &volume_arg,
        "worker",
        "/app/run.sh",
        &language_arg,
        &executable_arg,
    ];
    if !input_arg.is_empty() {
        args.push(&input_arg);
    }
    if !output_arg.is_empty() {
        args.push(&output_arg);
    }

    let output = Command::new("docker")
        .args(args)
        .output()
        .map_err(|e| Status::internal(format!("Docker 실행 실패: {}", e)))?;

    if !output.status.success() {
        return Err(Status::internal(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "[::]:50051".parse()?;
    let executor = MyExecutor::default();

    println!("서버 실행 중 @ {}", addr);

    Server::builder()
        .add_service(ExecutorServer::new(executor))
        .serve(addr)
        .await?;

    Ok(())
}
