# Client-Server RPC 구현

## 1. 개요

- 클라이언트와 서버 간의 원격 프로시저 호출(RPC) 구현

## 2. 사용법

- docker-compose up
- cargo build
- cargo run --bin server
- cargo run --bin client

```
💬 파일명을 입력하세요 (예: a.cpp, b.py):
b.py
💬 코드를 입력하세요 (입력 완료 후 Enter):
print('Hello World!')

실행 결과:
Hello World!
```

## 3. 구현

- 클라이언트
    - 서버로 파일명과 소스코드를 전송
- 서버
    - 파일을 생성
    - 워커에 소스코드 실행 요청
    - 실행 결과를 클라이언트에게 전송
- 워커
    - 소스코드 컴파일
    - 실행 결과를 서버에게 전송
