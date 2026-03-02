//! 统一错误类型，与详细设计 §10.1 一致

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    // 1xxx - 连接类
    #[error("SSH 连接超时: {0}")]
    SshTimeout(String),
    #[error("SSH 认证失败: {0}")]
    SshAuthFailed(String),
    #[error("ssh-agent 中未找到密钥: {0}")]
    SshKeyNotFound(String),

    // 2xxx - 预检类
    #[error("磁盘空间不足: 剩余 {0}GB，需要 ≥50GB")]
    DiskInsufficient(u64),
    #[error("内存不足: {0}GB，建议 ≥16GB")]
    MemoryLow(u64),
    #[error("OS 版本不支持: {0}")]
    UnsupportedOS(String),

    // 3xxx - 任务执行类
    #[error("Ansible Playbook 执行失败: {0}")]
    PlaybookFailed(String),
    #[error("同步停滞: {0} 分钟内区块高度未增长")]
    SyncStalled(u64),
    #[error("容器启动失败: {0}")]
    ContainerStartFailed(String),
    #[error("健康检查超时")]
    HealthCheckTimeout,

    // 4xxx - KES 类
    #[error("KES 证书格式无效")]
    InvalidKesCert,
    #[error("KES counter 不匹配: 期望 {expected}, 实际 {actual}")]
    KesCounterMismatch { expected: u64, actual: u64 },

    // 5xxx - 内部错误
    #[error("数据库错误: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("Sidecar 进程异常退出")]
    SidecarCrash,
    #[error("内部错误: {0}")]
    Internal(String),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
