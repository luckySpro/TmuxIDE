#!/bin/sh
# 发布产物冒烟测试：在干净环境中（无系统 tmux、无 git）跑通
# 新建会话 → 保存 → 推送到本地远程仓库 → 关闭 tmux → 恢复 → 在“另一台电脑”拉取。
# 用法：scripts/smoke.sh path/to/tmuxide
set -eu
BIN=$1
WORK=$(mktemp -d)
trap 'TMUXIDE_HOME=$WORK/a "$BIN" tmux kill-server >/dev/null 2>&1 || true; TMUXIDE_HOME=$WORK/b "$BIN" tmux kill-server >/dev/null 2>&1 || true' EXIT

for tool in tmux git; do
  if command -v "$tool" >/dev/null 2>&1; then echo "注意：环境中存在系统 ${tool}（$(command -v $tool)），本测试不会使用它"; fi
done

mkdir -p "$WORK/a" "$WORK/b"
for h in a b; do
  cat > "$WORK/$h/config.toml" <<CFG
[git]
remote = "$WORK/remote.git"
auth = "none"
auto_commit = true
[tmux]
socket = "smoke-$h-$$"
CFG
done

export TMUXIDE_HOME=$WORK/a
"$BIN" info
"$BIN" info --json | grep -q '"bundled": true' || { echo "没有使用内置 tmux"; exit 1; }
"$BIN" init-remote "$WORK/remote.git"

"$BIN" tmux new-session -d -s smoke -c /tmp -x 160 -y 48
"$BIN" tmux split-window -h -t smoke -c /tmp
"$BIN" tmux new-window -t smoke: -n logs -c /tmp
"$BIN" tmux send-keys -t smoke:logs 'sleep 600' Enter
sleep 1
"$BIN" save smoke
"$BIN" sync
"$BIN" tmux kill-server
sleep 1
"$BIN" restore smoke
test "$("$BIN" tmux list-windows -t smoke | wc -l | tr -d ' ')" = 2 || { echo "窗口数不对"; exit 1; }
test "$("$BIN" tmux list-panes -s -t smoke | wc -l | tr -d ' ')" = 3 || { echo "面板数不对"; exit 1; }
sleep 1
"$BIN" tmux list-panes -s -t smoke -F '#{window_name} #{pane_current_command}' | grep -q 'logs sleep' || { echo "启动命令没有恢复"; exit 1; }

export TMUXIDE_HOME=$WORK/b
"$BIN" sync
"$BIN" ls --json | grep -q '"name": "smoke"' || { echo "另一台电脑没有拉取到快照"; exit 1; }
echo "冒烟测试通过"
