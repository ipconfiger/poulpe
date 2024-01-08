use std::ops::ControlFlow;
use axum::extract::ws::{Message, WebSocket};
use std::{net::SocketAddr};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{channel, Sender};
use chrono::{DateTime, Utc};
use redis::aio::Connection;
use redis::AsyncCommands;
use tokio::sync::Mutex;
use crate::{AppState, TASK_DELAY, TASK_WORKING, WebTask};
use futures::{sink::SinkExt, stream::StreamExt};

#[derive(Clone)]
enum WsMsg {
    STR(String),
    BYT(Vec<u8>)
}

#[derive(Clone)]
pub struct MessageExecutor {
    sender_map: Arc<Mutex<HashMap<SocketAddr, Sender<WsMsg>>>>,
    req_map: Arc<Mutex<HashMap<String, SocketAddr>>>,
}

impl MessageExecutor {
    pub fn new()->MessageExecutor {
        MessageExecutor {
            sender_map: Arc::new(Mutex::new(HashMap::new())),
            req_map: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn bind_sender(&mut self, who: SocketAddr, sender: Sender<WsMsg>) {
        let mut sm = self.sender_map.lock().await;
        if let Some(rs) = sm.insert(who, sender){
        }
    }

    pub async fn clear_client(&mut self, who: SocketAddr) {
        let mut sm = self.sender_map.lock().await;
        if let Some(v) = sm.remove(&who) {
        }
    }

    pub async fn bind_request_id(&mut self, task_id: String, who: SocketAddr) {
        let mut req_map = self.req_map.lock().await;
        if req_map.contains_key(&task_id){
            req_map.remove(&task_id);
        }
        req_map.insert(task_id.clone(), who);
    }

    pub async fn response_for(&mut self, task_id: String, resp: String) {
        let who_is = {
            let req_map = self.req_map.lock().await;
            if let Some(who) = req_map.get(&task_id) {
                Some(who.clone())
            } else {
                None
            }
        };
        if let Some(the_who) = who_is {
            let mut sender_map = self.sender_map.lock().await;
            if let Some(mut sender) = sender_map.get(&the_who){
                sender.send(WsMsg::STR(resp));
                sender_map.remove(&the_who);
            }
        }
    }


}


pub async fn handle_socket(mut state: AppState, mut socket: WebSocket, who: SocketAddr) {
    let mut redis_conn = state.redis_client.get_async_connection().await.expect("Redis连接失败");
    let (mut sender, mut receiver) = socket.split();
    let (mpsc_tx, mpsc_rx) = channel::<WsMsg>();
    state.ws_executor.bind_sender(who, mpsc_tx).await;

    let mut send_task = tokio::spawn(async move {
        loop {
            if let Ok(msg) = mpsc_rx.recv() {
                if let WsMsg::STR(str_resp) = msg {
                    if sender.send(Message::Text(str_resp)).await.is_err() {
                       println!("向ws client写入消息错误");
                        return 1;
                    }
                }
            }else{
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }
    });

    let mut recv_task = tokio::spawn(async move {
        let mut cnt = 0;
        while let Some(Ok(msg)) = receiver.next().await {
            cnt += 1;
            // print message and break if instructed to do so
            if process_message(msg, who, &mut state, &mut redis_conn).await.is_break() {
                break;
            }
        }
        cnt
    });
    tokio::select! {
        rv_a = (&mut send_task) => {
            match rv_a {
                Ok(a) => println!("{a} messages sent to {who}"),
                Err(a) => println!("Error sending messages {a:?}")
            }
            recv_task.abort();
        },
        rv_b = (&mut recv_task) => {
            match rv_b {
                Ok(b) => println!("Received {b} messages"),
                Err(b) => println!("Error receiving messages {b:?}")
            }
            send_task.abort();
        }
    }
}

async fn process_message(msg: Message, who: SocketAddr, state: &mut AppState, redis_conn: &mut Connection) -> ControlFlow<(), ()> {
    let msg_need_to_process = match msg {
        Message::Text(t) => Some(t),
        Message::Close(c) => {
            if let Some(cf) = c {
                println!(">>> {} sent close with code {} and reason `{}`", who, cf.code, cf.reason);
            } else {
                println!(">>> {who} somehow sent close message without CloseFrame");
            }
            return ControlFlow::Break(());
        }
        Message::Pong(v) => None,
        Message::Ping(v) => None,
        _=>None
    };
    if let Some(msg_str) = msg_need_to_process {
        if let Ok(mut web_task) = serde_json::from_str::<WebTask>(msg_str.as_str()) {
            let mut task = web_task.gen_task(state.queue.size);
            let now: DateTime<Utc> = Utc::now();
            let now_ts = now.timestamp();
            task.src_chn = "ws".to_string();
            let redis_payload = serde_json::to_string(&task).unwrap();
            redis_conn.set::<String, String, ()>(task.id.clone(), redis_payload).await.expect("set error");
            state.ws_executor.bind_request_id(task.id.clone(), who.clone()).await;
            if web_task.delay == 0 {
                redis_conn.sadd::<String, String, ()>(TASK_WORKING.to_string(), task.id.clone()).await.expect("set list error");
                println!("开始分配Worker线程");
                state.queue.dispatch_task(&task);
            } else {
                let delay_key = format!("{} {}", task.id, now_ts + web_task.delay as i64);
                redis_conn.sadd::<String, String, ()>(TASK_DELAY.to_string(), delay_key.clone()).await.expect("set list error");
            }
        }
    }
    ControlFlow::Continue(())
}