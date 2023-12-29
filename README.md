# poulpe : /pulp/
Light weight easy to use task manager written by Rust

使用Rust实现的轻量级作业管理系统。

支持的作业模式有：

1. 定时任务（秒级粒度）
2. 延迟触发任务
3. 即时触发任务

作业执行的类型支持：

1. HTTP（GET｜POST）
2. 邮件
3. 服务端本地脚本

例子：

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


实时触发和延迟触发均通过http接口

    curl -X POST http://127.0.0.1:8000/task_in_queue
         -H 'Content-Type: application/json'
         -d '{...}'

JSON Body的格式如下：

    {
        "id": "唯一编码",
        "method": "作业执行的类型 GET|POST|EXEC|MAIL_TO",
        "delay": 0,       # 延迟执行的秒数，0为即时触发
        "name": "命令名",  # GET|POST的时候是请求地址，EXEC为命令名，MAIL_TO为邮件名
        "params": "参数",  # GET时为QueryString，POST时是JSON字符串，EXEC时为空格隔开的参数，MAIL_TO的时候为专门定义的JSON字符串
        "cc": 3,          # 支持并行执行的数量，0为不限制
        "cf": "test"      # 并行标识，相同标识的处于相同的并行队列
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

