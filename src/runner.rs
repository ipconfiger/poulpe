use serde::{Deserialize, Serialize};
use reqwest::blocking::Client;
use lettre::{Message, SmtpTransport, Transport};
use lettre::message::{header::ContentType};
use lettre::transport::smtp::authentication::Credentials;
use std::process::Command;

#[derive(Clone)]
pub struct AppConfig {
    pub smtp_server: String,
    pub smtp_port: i32,
    pub smtp_name: String,
    pub smtp_pwd: String,
    pub retry_interval: i32,
    pub max_retry: i32,
    pub starttls: bool
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Mail {
    from_addr: String,
    to_addr: String,
    title: String,
    body: String
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Task {
    pub id: String,
    pub tk_tp: String,
    pub delay: i64,
    pub name: String,
    pub params: String,
    pub post_time: i64,
    pub exec_time: i64,
    pub retry: i32,
    pub cc: Vec<i32>,
    pub error: String
}

impl Task {
    pub fn execute(&mut self, config: &AppConfig) -> Result<(), String>{
        if self.tk_tp.to_lowercase() == "get" {
            return http_get(self);
        }
        if self.tk_tp.to_lowercase() == "post" {
            return http_post(self);
        }
        if self.tk_tp.to_lowercase() == "exec" {
            return exec(self);
        }
        if self.tk_tp.to_lowercase() == "mail_to" {
            return mail_to(self, config);
        }
        Ok(())
    }
}

fn http_get(task: &Task) -> Result<(), String> {
    let client = Client::new();
    let url = format!("{}?{}", task.name.clone(), task.params.clone());
    if let Ok(resp) = client.get(url).send() {
        println!("Get Resp:{:?}", resp);
        if resp.status() != 200 {
            return Err("服务端错误".to_string());
        }
        return Ok(());
    } else {
        println!("没有Response");
        return Err("URL无法访问".to_string());
    }
}

fn http_post(task: &Task) -> Result<(), String> {
    let client = Client::new();
    if let Ok(resp) = client.post(task.name.clone()).body(task.params.clone()).header("content-type", "application/json").send() {
        if resp.status() != 200 {
            return Err("服务端错误".to_string());
        }
        return Ok(());
    }else{
        return Err("URL无法访问".to_string());
    }
}

fn exec(task: &Task) -> Result<(), String> {
    let params = task.params.split_whitespace().collect::<Vec<_>>();
    let output = Command::new(task.name.clone())
        .args(&params)
        .output().expect("Run faild");
    println!("exec with status:{}", output.status);
    if let Some(code) = output.status.code(){
        if code > 0{
            let error = String::from_utf8_lossy(&output.stdout);
            println!("error:{}", error);
            let converted = format!("{}", error.replace("\n", "<br>"));
            Err(format!("执行指令:{} {}<br>报错信息:<br>{}",task.name.clone(), task.params.clone(), converted))
        }else{
            println!("output:{}", String::from_utf8(output.stdout).unwrap());
            Ok(())
        }
    }else{
        Err("执行失败".to_string())
    }
}

fn mail_to(task: &Task, config: &AppConfig) -> Result<(), String> {
    if let Ok(mail) = serde_json::from_str::<Mail>(&task.params) {
        let email = Message::builder()
            .from(mail.from_addr.parse().expect(" 非法发件地址"))
            .to(mail.to_addr.parse().expect("非法收件地址"))
            .subject(mail.title.clone())
            .header(ContentType::TEXT_PLAIN)
            .body(mail.body.clone())
            .expect("Invalid mail");
        let smtp_result = if config.starttls {
            SmtpTransport::starttls_relay(config.smtp_server.as_str())
        } else {
            SmtpTransport::relay(config.smtp_server.as_str())
        };
        match smtp_result {
            Ok(smtp_transport)=>{
                println!("creating smtp transport ok!");
                let transport = smtp_transport.credentials(Credentials::new(config.smtp_name.clone(), config.smtp_pwd.to_string()))
                    .port(config.smtp_port as u16)
                    .timeout(Some(std::time::Duration::from_secs(5)))
                    .build();
                println!("will send mail to:{}", mail.to_addr.clone());
                let result = transport.send(&email);
                match result {
                    Ok(_) => {
                        println!("Email sent successfully!");
                        Ok(())
                    },
                    Err(e) => {
                        println!("Could not send email: {e:?}");
                        Err(format!("Could not send email: {e:?}"))
                    },
                }
            }
            Err(ex)=>{
                println!("create transport faild with err:{:?}", ex);
                Err(format!("create transport faild with err:{:?}", ex))
            }
        }
    }else{
        return Err("邮件任务参数不正确".to_string());
    }
}