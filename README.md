# poulpe : /pulp/

![octopus](https://github.com/ipconfiger/poulpe/assets/950968/aa364b2f-9429-40c1-b110-2d29f033e213)


Light weight easy to use task manager written by Rust

A lightweight job management system implemented in Rust. It is designed to simplify the development of task scheduling in online systems, supporting second-level granularity for timed task triggering, as well as immediate and delayed task scheduling. At this stage, it supports triggering HTTP requests, local server program calls, email sending, and other types of tasks. Tasks are executed in multiple pre-started worker threads, with support for controlling the concurrency capabilities of tasks. In this way, developers can focus on the business, without having to worry about technical details such as multithreading, multiprocessing, concurrency, thread isolation, process calls, error retries, task persistence, and recovery of tasks after restart. Developed in Rust, the released version is compiled with Musl for the x86 architecture, and consists of a single executable file that is ready to use out of the box. Musl compilation ensures that the program has no unnecessary dependencies and can be executed in minimal Docker images such as Alpine, making deployment convenient and fast.

使用Rust实现的轻量级作业管理系统。用于简化线上系统的任务调度开发，支持秒级粒度的定时任务触发，支持即时任务调度以及延迟任务调度。现阶段支持触发HTTP请求，服务端本地程序调用，邮件发送等任务类型。任务执行在多个预启动的工作线程中，支持对任务的并发能力进行控制。如此这般，开发人员可以将精力集中于业务，而不需要关心多线程，多进程，并发，线程隔离，进程调用，错误重试，任务持久化，重启任务的恢复等技术细节。采用Rust开发，发布的基于x86架构的Musl编译版本，只有一个可执行文件，开箱即用，Musl编译使得程序没有任何多余的依赖，可以在Apline等最小化的Docker镜像中执行，部署方便快捷。

The supported job modes include:

1. Scheduled tasks (second-level granularity)
2. Delayed trigger tasks
3. Immediate trigger tasks

Supported types of job execution include:

1. HTTP (GET | POST)
2. Email
3. Server-side local scripts

支持的作业模式有：

1. 定时任务（秒级粒度）
2. 延迟触发任务
3. 即时触发任务

作业执行的类型支持：

1. HTTP（GET｜POST）
2. 邮件
3. 服务端本地脚本

例子（Sample）：

    ./poulpe \
        --port 8000               #运行的端口
        --redis redis://127.0.0.1 #用于持久化作业的redis连接
        --cron /etc/cron_task     #定义定时任务的文件地址
        --dead /tmp/deadpool      #死信箱目录，用于存储死任务
        --workers 4               #启动工作线程的数量
        --retry_interval 10       #错误重试间隔时间
        --max_retry 3             #最多重试的次数
        --smtp_server xxx.xxx.com #SMTP服务器地址
        --smtp_port 23            #SMTP服务器端口
        --smtp_name alex          #SMTP账号
        --smtp_pwd *****          #SMTP密码
        --starttls                #是否启用starttls
        --kafka_servers           #Kafka的Broker列表，支持接入群集
        --kafka_topic             #接收任务的Topic
        --kafka_resp_topic        #返回任务执行结果的Topic

| 参数（parameter） |说明| Description                                        |
|---------------| ------------- |----------------------------------------------------|
| port          |运行端口| port for http server                               |
| redis         |用于持久化作业的redis连接| Redis connection used for job persistence.         |
| cron          |定义定时任务的文件地址| File path for defining scheduled tasks             |
| dead          |死信箱目录，用于存储死任务| Dead letter directory, used for storing dead tasks |


实时触发和延迟触发均支持 HTTP 和 Kafka 多通道接入，默认只开启HTTP，在设置好正确的kafka前缀的参数后，即可从Kafka接收任务

    http 接收任务的例子：
    curl -X POST http://127.0.0.1:8000/task_in_queue
         -H 'Content-Type: application/json'
         -d '{...}'

HTTP请求的JSON Body和Kafka的消息题的格式是一致的，都是JSON格式，定义如下：

    {
        "id": "唯一编码, 任务的唯一编号",
        "method": "作业执行的类型 GET|POST|EXEC|MAIL_TO",
        "delay": 0,       # 延迟执行的秒数，0为即时触发
        "name": "命令名",  # GET|POST的时候是请求地址，EXEC为命令名，MAIL_TO为邮件名
        "params": "参数",  # GET时为QueryString，POST时是JSON字符串，EXEC时为空格隔开的参数，MAIL_TO的时候为专门定义的JSON字符串
        "cc": "1 3",      # 空格隔开的指定线程编号，这里指定两个线程，表示最多可以并行执行2个任务，留空表示不限制
        "wait": 0         # 可选，大于0表示需要在提交任务后等待执行的结果，数值为等待超时时间
    }

MAIL_TO 的params的JSON结构

    {
        "from_addr": "发件地址",
        "to_addr": "收件地址",
        "title": "邮件标题",
        "body": "邮件正文"
    }

暂时不支持附件和html

CRON配置文件格式

    秒 分 时 日  月 星期 年  命令name medod 参数
    1  *  *  *  *  *  *     cmd     GET     xx xx xx xx

cron不支持MAIL_TO

### 获取待执行结果：

系统提供long pulling获取执行结果的接口，以提供给需要更长执行时间的任务用于获取执行结果：比如压缩视频

    curl http://127.0.0.1:8000/task_resp/任务编号

### 获取系统当前状态

通过一个GET请求获取当前运行状态，包括正在执行的任务id列表，等待执行的延迟任务id列表，执行报错任务id列表，以及各个工作线程的执行队列长度信息

    curl http://127.0.0.1:8000/sys_info

返回JSON对象，方便和运维工具集成，比如队列过长的时候告警