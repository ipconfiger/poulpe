mod runner;

extern crate redis;
use std::sync::{Arc, Mutex};
use std::collections::VecDeque;
use clap::{App, Arg, ArgMatches};
use redis::{AsyncCommands, Commands};
use std::net::SocketAddr;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use axum::{extract::{Json, State}, Router, routing::post, response::IntoResponse};
use std::{env, thread};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tokio::fs::File;
use tokio::io::{self, BufReader, AsyncBufReadExt};
use chrono::{DateTime, Utc, Local};
use serde::{Deserialize, Serialize};
use tokio::fs;
use runner::{Task, AppConfig};
use dirs;



#[derive(Clone)]
pub struct AppState {
    /// 配置
    pub config_path: String,
    pub redis_client: redis::Client,
    pub queue: Arc<Mutex<VecDeque<String>>>,
    pub config: AppConfig
}

#[derive(Serialize, Deserialize, Debug)]
struct WebTask {
    id: String,
    method: String,
    delay: i32,
    name: String,
    params: String,
    cf: String,
    cc: i32
}

struct LastModifyHolder {
    last_modify: String,
    tasks: Vec<String>,
    avaliables: Vec<Task>
}

const TASK_WRONG: &'static str = "task||wrong";

const TASK_WORKING: &'static str = "task||working";

const TASK_DELAY: &'static str = "task||delayed";

fn matches_config(matches: App) -> ArgMatches{
    matches.arg(Arg::with_name("port")
        .short('p')
        .long("port")
        .help("服务器运行端口")
        .required(true)
        .takes_value(true)
        .default_value("9527"))
        .arg(Arg::with_name("redis")
            .short('r')
            .long("redis")
            .help("Redis连接URL")
            .required(true)
            .takes_value(true))
        .arg(Arg::with_name("cron")
            .short('c')
            .long("cron")
            .help("cron tab 文件地址")
            .required(true)
            .takes_value(true))
        .arg(Arg::with_name("workers")
            .short('w')
            .long("workers")
            .help("启动工作线程数量")
            .required(false)
            .default_value("4")
            .takes_value(true))
        .arg(Arg::with_name("smtp_server")
            .short('s')
            .long("smtp_server")
            .help("外发SMTP服务器地址")
            .takes_value(true)
            .default_value(""))
        .arg(Arg::with_name("smtp_port")
            .short('x')
            .long("smtp_port")
            .help("外发SMTP端口号")
            .takes_value(true)
            .default_value("465"))
        .arg(Arg::with_name("smtp_name")
            .short('n')
            .long("smtp_name")
            .help("外发SMTP账号")
            .takes_value(true)
            .default_value(""))
        .arg(Arg::with_name("smtp_pwd")
            .short('y')
            .long("smtp_pwd")
            .help("外发SMTP密码")
            .takes_value(true)
            .default_value(""))
        .arg(Arg::with_name("retry_interval")
            .short('i')
            .long("retry_interval")
            .help("错误重试的间隔")
            .takes_value(true)
            .default_value("10"))
        .arg(Arg::with_name("max_retry")
            .short('m')
            .long("max_retry")
            .help("最大重试次数")
            .takes_value(true)
            .default_value("3"))
        .arg(Arg::with_name("starttls")
            .short('k')
            .long("starttls")
            .help("使用SSL")
            .action(clap::ArgAction::SetTrue))
        .arg(Arg::with_name("dead")
            .short('d')
            .long("dead")
            .help("死信箱地址")
            .required(false)
            .default_value("/tmp/deadpool")
            .takes_value(true)).get_matches()
}

fn config_from_matches(matches: &ArgMatches) -> AppConfig {
    let smtp_name = matches.get_one::<String>("smtp_name").unwrap();
    let smtp_pwd = matches.get_one::<String>("smtp_pwd").unwrap();
    let retry_interval = matches.get_one::<String>("retry_interval").unwrap().parse::<i32>().unwrap();
    let max_retry = matches.get_one::<String>("max_retry").unwrap().parse::<i32>().unwrap();
    let smtp_server = matches.get_one::<String>("smtp_server").unwrap();
    let smtp_port = matches.get_one::<String>("smtp_port").unwrap().parse::<i32>().unwrap();
    let starttls = matches.get_flag("starttls");

    AppConfig{
        smtp_name: smtp_name.to_string(),
        smtp_pwd:smtp_pwd.to_string(),
        retry_interval:retry_interval,
        max_retry:max_retry,
        smtp_server: smtp_server.to_string(),
        smtp_port: smtp_port,
        starttls: starttls
    }
}

#[tokio::main]
async fn main() {
    // 读取配置
    let app = App::new("Poulpe Task Management");
    let matches = matches_config(app);

    let port = matches.value_of("port").unwrap();
    let int_port = port.parse().unwrap();
    let redis = matches.value_of("redis").unwrap();
    let cron_path = matches.value_of("cron").unwrap();
    let dead_base = matches.get_one::<String>("dead").map(|s| s.to_string()).unwrap();
    // 将拥有自己数据的 String 转换为 Arc 类型，以便在其他线程中共享所有权
    let dead_base_arc = Arc::new(dead_base);
    // 在其他线程中使用 Arc 类型的字符串
    let thread_arc_dead_base = Arc::clone(&dead_base_arc);

    let appconfig = config_from_matches(&matches);

    let workers = matches.value_of("workers").unwrap().parse().unwrap();
    let queue: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));
    let client = redis::Client::open(redis).unwrap();

    for thread_id in 0..workers {
        let queue_ref = queue.clone();
        let mut redis_connection = client.get_connection().unwrap();
        let appconfig = appconfig.clone();
        thread::spawn(move || {
            println!("worker:{} started", thread_id);
            loop {
                let task_id = {
                    let mut queue = queue_ref.lock().unwrap();
                    if queue.is_empty() {
                        "".to_string()
                    }else{
                        queue.pop_front().unwrap()
                    }
                };
                if task_id == "" {
                    thread::sleep(std::time::Duration::from_secs(1));
                    continue;
                }
                println!("worker:{} 捕获任务", task_id.clone());
                if let Ok(task_str) = redis_connection.get::<String, String>(task_id.clone()) {
                    println!("线程[{}]获取到任务:{} 的数据：{}", thread_id, task_id.clone(), task_str);
                    if let Ok(mut task) = serde_json::from_str::<Task>(task_str.as_str()){
                        // 执行任务
                        let if_err = match task.execute(&appconfig) {
                            Ok(())=>{"".to_string()},
                            Err(err_str)=>{err_str}
                        };
                        if !if_err.is_empty() {
                            // 错误处理逻辑
                            task.error = if_err.to_string();
                            println!("执行错误：{}", task.error);
                            if let Ok(save_payload) = serde_json::to_string(&task) {
                                redis_connection.set::<String, String, ()>(task.id.clone(), save_payload).expect("回存变更错误");
                            }
                            redis_connection.sadd::<&str, String, ()>(TASK_WRONG, task.id.clone()).expect("添加到错误队列错误")
                        }else{
                            redis_connection.del::<&str, ()>(task.id.as_str()).expect("redis del error");
                        }
                        //从正在执行队列中去掉
                        redis_connection.srem::<&str, String, ()>(TASK_WORKING, task.id.clone()).expect("redis error");
                        if !task.cf.is_empty() {
                            let cc_flag = format!("cc:{}", task.cf);
                            if let Ok(cc) = redis_connection.get::<String, String>(cc_flag.clone()) {
                                let new_cc = if let Ok(int_cc) = cc.parse::<i32>() { if int_cc>0 { int_cc - 1}else { 0 } } else { 0 };
                                redis_connection.set::<String, String, ()>(cc_flag.clone(), format!("{}", new_cc)).expect("设置并行标识失败");
                            }
                        }
                    }
                }else{
                    println!("任务:{} 不存在", task_id.clone());
                }
            }
        });
    }

    let mut conn_err = client.get_async_connection().await.unwrap();
    let queue_err = queue.clone();

    tokio::spawn(async move {
        println!("错误补发线程启动");
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(appconfig.retry_interval as u64));
        loop {
            interval.tick().await;
            println!("尝试获取错误的任务");
            if let Ok(err_keys) = conn_err.smembers::<&str, Vec<String>>(TASK_WRONG).await {
                println!("got error keys:{:?} in 10's", err_keys);
                for tk_key in err_keys {
                    if let Ok(err_tk_str) = conn_err.get::<String, String>(tk_key.clone()).await {
                        if let Ok(mut tk) = serde_json::from_str::<Task>(err_tk_str.as_str()){
                            conn_err.srem::<&str, String, ()>(TASK_WRONG, tk.id.clone()).await.expect("从出错队列删除错误");
                            if tk.retry > appconfig.max_retry - 1 {
                                conn_err.del::<String, ()>(tk.id.clone()).await.expect("删除错误");
                                // 超过重试总次数，从错误队列删除，写入死信箱
                                if let Ok(save_str) = serde_json::to_string_pretty(&tk) {
                                    write_dead_pool(thread_arc_dead_base.to_string(), &tk.id, &save_str).await;
                                }
                            }else{
                                tk.retry +=1;
                                if let Ok(save_str) = serde_json::to_string(&tk){
                                    conn_err.set::<String, String, ()>(tk.id.clone(), save_str).await.expect("回写重试次数出错");
                                    conn_err.sadd::<String, String, ()>(TASK_WORKING.to_string(), tk.id.clone()).await.expect("set list error");
                                    let mut queue4 = queue_err.lock().unwrap();
                                    queue4.push_back(tk.id.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
    });


    let str_cron_path = get_abs_path(cron_path.to_string());
    let mut conn = client.get_async_connection().await.unwrap();
    let queue2 = queue.clone();
    tokio::spawn(async move {
        let mut holder = LastModifyHolder{last_modify:"".to_string(), tasks: vec![], avaliables: vec![]};
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            let now: DateTime<Utc> = Utc::now();
            let now_ts = now.timestamp();
            let move_cron_path = str_cron_path.as_str();
            let buf_cron_path = PathBuf::from(move_cron_path);
            let last_modify = crontab_file_changed(buf_cron_path.clone(), holder.last_modify.as_str()).await;
            if last_modify != "" {
                holder.last_modify = last_modify;
                println!("last modify:{} 有变更，重新加载", holder.last_modify);
                read_crontab_file(buf_cron_path.clone(), &mut holder.tasks).await.unwrap();
            }
            get_tasks_avaliable(&holder.tasks, &mut holder.avaliables);
            let mut will_exec: Vec<String> = vec![];
            if holder.avaliables.len() > 0 {
                for task in &holder.avaliables {
                    will_exec.insert(0, task.id.clone());
                    let payload = serde_json::to_string_pretty(task).unwrap();
                    println!("task payload:\n{}", payload);
                    conn.set::<String, String, ()>(task.id.clone(), payload.clone()).await.expect("set error");
                }
            }
            if let Ok(delayed) = conn.smembers::<&str, Vec<String>>(TASK_DELAY).await {
                for delay_key in delayed {
                    let delay_seq = delay_key.split_whitespace().collect::<Vec<_>>();
                    let tar_ts:i64 = delay_seq[1].parse().unwrap();
                    if tar_ts == now_ts {
                        println!("监测到可执行延迟操作：{}", delay_seq[0]);
                        will_exec.insert(0, delay_seq[0].to_string());
                        conn.srem::<&str, String, ()>(TASK_DELAY, delay_key.clone()).await.expect("删除错误");
                    }else{
                        if tar_ts < now_ts {
                            conn.srem::<&str, String, ()>(TASK_DELAY, delay_key.clone()).await.expect("删除错误");
                        }
                    }
                }
            }
            for task_key in will_exec{
                conn.sadd::<String, String, ()>(TASK_WORKING.to_string(), task_key.clone()).await.expect("set list error");
                let mut queue3 = queue2.lock().unwrap();
                queue3.push_back(task_key.clone());
            }
        }
    });

    let app_state = AppState{
        config_path: cron_path.to_string(),
        redis_client: client.clone(),
        queue: queue.clone(),
        config: appconfig.clone()
    };
    // 创建HTTP路由
    let app = Router::new()
        .route("/task_in_queue", post(handler))
        .with_state(app_state);

    // 启动HTTP服务器
    println!("server will start at 0.0.0.0:{}", port);
    let serv = axum::Server::bind(& SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), int_port))
        .serve(app.into_make_service())
        .await;
    match serv {
        Ok(_)=>{
            println!("server stoped normally");
        }
        Err(err)=>{
            println!("server failt with err:{}", err);
        }
    }
}

pub async fn handler(State(state): State<AppState>,
                     Json(payload): Json<serde_json::Value>
) -> impl IntoResponse {
    let now: DateTime<Utc> = Utc::now();
    let now_ts = now.timestamp();
    let mut conn = state.redis_client.get_async_connection().await.unwrap();
    if let Ok(web_task) = serde_json::from_value::<WebTask>(payload.clone()) {
        if state.config.smtp_name.is_empty() && web_task.method.to_lowercase() == "mail_to" {
            return Json(serde_json::json!({"result":"Fail", "reason": "未配置SMTP服务器"}));
        }
        // concurrency 控制 只对即时任务有效
        if web_task.cc > 0 && web_task.delay == 0 {
            let cc_flag = format!("cc:{}", web_task.cf);
            let cc_now = if let Ok(v) = conn.get::<String, String>(cc_flag.clone()).await { v.parse().unwrap() } else { 0 };
            println!("当前流控并发:{}", cc_now);
            if cc_now >= web_task.cc {
                return Json(serde_json::json!({"result":"Over", "reason": "超标限流"}));
            }else{
                println!("流控+1");
                conn.set::<String, String, ()>(cc_flag.clone(), format!("{}", cc_now + 1)).await.expect("增量错误");
            }
        }

        let task_key = format!("task-{}", web_task.id.clone());
        let task = Task{
            id:task_key.clone(),
            tk_tp:web_task.method.clone(),
            delay: web_task.delay as i64,
            name: web_task.name.clone(),
            params: web_task.params.clone(),
            post_time: now_ts,
            exec_time: 0,
            retry: 0,
            cf: web_task.cf,
            error: "".to_string()
        };
        let redis_payload = serde_json::to_string(&task).unwrap();
        conn.set::<String, String, ()>(task_key.clone(), redis_payload).await.expect("set error");
        if web_task.delay == 0 {
            conn.sadd::<String, String, ()>(TASK_WORKING.to_string(), task_key.clone()).await.expect("set list error");
            let mut queue = state.queue.lock().unwrap();
            queue.push_back(task_key);
        } else {
            let delay_key = format!("{} {}", task_key, (now_ts + web_task.delay as i64));
            conn.sadd::<String, String, ()>(TASK_DELAY.to_string(), delay_key.clone()).await.expect("set list error");
        }
        Json(serde_json::json!({"result":"OK", "reason": ""}))
    }else{
        Json(serde_json::json!({"result":"Fail", "reason": format!("非法的请求:{}", payload.clone())}))
    }
}

async fn read_crontab_file(config_path: PathBuf, tasks: &mut Vec<String>) -> io::Result<()> {
    let file = File::open(config_path).await?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    tasks.clear();
    while let Some(line) = lines.next_line().await? {
        if line.starts_with("#"){
            continue;
        }
        tasks.push(line);
    }
    Ok(())
}

async fn crontab_file_changed(file_path: PathBuf, last_dt: &str) -> String {
    if let Ok(metadata) = fs::metadata(file_path.clone()).await {
        if let Ok(last_modified) = metadata.modified() {
            let last_modified_str = format!("{:?}", last_modified);
            if last_modified_str != last_dt {
                return last_modified_str;
            }
        }else{
            println!("从meta获取last modify失败");
        }
    }else{
        println!("获取meta错误：{}", file_path.to_str().unwrap());
    }
    "".to_string()
}

fn get_abs_path(address: String) -> String{
    let absolute_path = if Path::new(&address).is_relative() {
        if address.starts_with("~"){
            if let Some(mut path) = dirs::home_dir() {
                path.push( &address[2..].to_string());
                return path.to_str().unwrap().to_string();
            }else{
                return address;
            }
        }else{
            let current_dir = env::current_dir().expect("Failed to get current directory");
            let mut path_buf = PathBuf::from(current_dir);
            path_buf.push(address);
            path_buf
        }
    } else {
        PathBuf::from(address)
    };
    return absolute_path.to_str().unwrap().to_string();
}

fn get_tasks_avaliable(tasks: &Vec<String>, avaliable_tasks: &mut Vec<Task>) {
    avaliable_tasks.clear();
    let now: DateTime<Utc> = Utc::now();
    let mut sequence= 0;
    for line in tasks {
        sequence+=1;
        let time_config = line.split_whitespace().take(7).collect::<Vec<_>>().join(" ");
        let command = line.split_whitespace().skip(7).collect::<Vec<_>>().join(" ");
        if let Ok(cron) = cron::Schedule::from_str(&time_config) {
            if let Some(next) = cron.upcoming(Local).next() {
                let next_ts = next.timestamp() - 1;
                let now_ts = now.timestamp();
                if next_ts == now_ts {
                    // execute command
                    let command_id = format!("ST:{}-{}", sequence, now_ts);
                    let command_seqs = command.split_whitespace().collect::<Vec<_>>();
                    let name = command_seqs[0];
                    let method = command_seqs[1];
                    let params = command_seqs.iter().skip(2).map(|s| *s).collect::<Vec<_>>().join(" ");
                    let task = Task{id: command_id,
                        tk_tp:method.to_string(),
                        delay:0,
                        name:name.to_string(),
                        params: params.to_string(),
                        post_time:now_ts,
                        exec_time:0,
                        retry:0,
                        cf: "".to_string(),
                        error: "".to_string()
                    };
                    avaliable_tasks.insert(0, task);
                    //println!("cmd:{} methid:{} with:{}", name, method, params);
                }else{
                    //println!("cron:{} next:{}, current:{}", time_config, next_ts, now_ts);
                }
            }
        }
    }
}

async fn write_dead_pool(base_dir: String, tk_key: &String, tks: &String) {
    let abs_base = get_abs_path(base_dir);
    let now: DateTime<Utc> = Utc::now();
    let dts = now.to_string().split_whitespace().collect::<Vec<_>>()[0].to_string();
    let mut base_path = PathBuf::from(abs_base);
    base_path.push(dts);
    tokio::fs::create_dir_all(&base_path).await.expect("无法创建死信箱目录");
    base_path.push(format!("{}.json", tk_key));
    tokio::fs::write(base_path, tks.as_str()).await.expect("写入死信箱失败");
}