function cleanMessage(raw: string): string {
  let value = raw.trim();
  const prefixes = ["Error:", "内部错误:"];
  let changed = true;
  while (changed) {
    changed = false;
    for (const prefix of prefixes) {
      if (value.startsWith(prefix)) {
        value = value.slice(prefix.length).trim();
        changed = true;
      }
    }
  }
  return value.replace(/\s+/g, " ").trim();
}

export function formatUserErrorMessage(raw: string): string {
  const original = raw.trim();
  const value = cleanMessage(original);
  const lower = original.toLowerCase();

  if (lower.includes("permission denied while trying to connect to the docker daemon socket")) {
    if (lower.includes("network.socket.connect") && lower.includes("does not exist")) {
      return "当前 SSH 用户无法直接访问 Docker daemon，且节点 socket 仍未就绪。请为该用户配置 docker 权限或 passwordless sudo，并确认节点仍在启动或恢复数据。";
    }
    return "当前 SSH 用户无法直接访问 Docker daemon。请为该用户配置 docker 权限或 passwordless sudo。";
  }
  if (lower.includes("missing sudo password")) {
    return "目标机需要 sudo 密码，但当前链路只支持 root 登录或 passwordless sudo。";
  }
  if (lower.includes("no keys in ssh-agent")) {
    return "ssh-agent 中没有可用私钥。请先添加 SSH 私钥后再继续。";
  }
  if (lower.includes("command ssh_agent_add_key missing required key keypath")) {
    return "缺少 SSH 私钥路径。请填写 keyPath 后再添加到 ssh-agent。";
  }
  if (lower.includes("genesis_verification_key") && lower.includes("mandatory")) {
    return "Mithril 缺少必填验证参数 genesis_verification_key。请检查 restore_snapshot 配置和验证密钥。";
  }
  if (lower.includes("unpack directory 'db' is not empty")) {
    return "Mithril 恢复要求空数据库目录。请清空 /opt/cardano/db 后重试，或关闭 restore_snapshot。";
  }
  if (lower.includes("network.socket.connect") && lower.includes("does not exist")) {
    return "节点 socket 尚未就绪。当前节点可能仍在启动、恢复快照或重放数据库。";
  }
  if (lower.includes("ansible playbook 执行失败")) {
    return value;
  }
  if (lower.includes("ssh command failed")) {
    return value;
  }
  return value;
}

export function toUserError(error: unknown): string {
  if (error instanceof Error) {
    return formatUserErrorMessage(error.message);
  }
  return formatUserErrorMessage(String(error));
}

export function formatTaskError(error: string | null | undefined): string | null {
  if (!error) {
    return null;
  }
  return formatUserErrorMessage(error);
}
