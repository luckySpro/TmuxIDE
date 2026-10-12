#!/bin/sh
# 远程访问冒烟测试：开启远程访问 → 等待隧道上线 → 经公网地址完成配对、认证和终端连接。
# 用法：scripts/remote-smoke.sh path/to/tmuxide        （默认 Cloudflare 快速隧道，需要外网）
#       MODE=local scripts/remote-smoke.sh path/to/tmuxide   （只测本机端口）
set -eu
BIN=$1
MODE=${MODE:-cloudflare-quick}
WORK=$(mktemp -d)
PORT=$((20000 + $$ % 20000))
RPORT=$((PORT + 1))
export TMUXIDE_HOME=$WORK/home
mkdir -p "$TMUXIDE_HOME"
cat > "$TMUXIDE_HOME/config.toml" <<CFG
[tmux]
socket = "remote-smoke-$$"
[remote]
enabled = true
mode = "$MODE"
port = $RPORT
require_pin = false
keep_awake = false
CFG

"$BIN" serve --no-open --port "$PORT" > "$WORK/serve.log" 2>&1 &
SERVER=$!
cleanup() {
  kill "$SERVER" 2>/dev/null || true
  wait "$SERVER" 2>/dev/null || true
  "$BIN" tmux kill-server >/dev/null 2>&1 || true
}
trap cleanup EXIT
fail() { echo "失败：$*"; echo "--- 服务日志"; cat "$WORK/serve.log"; exit 1; }
json() { python3 -c "import json,sys; d=json.load(sys.stdin); print(eval(sys.argv[1]))" "$1"; }

for _ in $(seq 1 50); do grep -q 'token=' "$WORK/serve.log" && break; sleep 0.2; done
TOKEN=$(grep -o 'token=[0-9a-f]*' "$WORK/serve.log" | head -1 | cut -d= -f2)
[ -n "$TOKEN" ] || fail "服务没有启动"
L="http://127.0.0.1:$PORT/api"
local_api() { curl -fsS -H "x-tmuxide-token: $TOKEN" -H 'content-type: application/json' "$@"; }

"$BIN" tmux new-session -d -s smoke -x 100 -y 30

# 1. 等待上线
URL=
for _ in $(seq 1 90); do
  S=$(local_api "$L/remote/events" || true)
  STATE=$(echo "$S" | json "d['state']" 2>/dev/null || echo "?")
  if [ "$STATE" = online ]; then URL=$(echo "$S" | json "d['url']"); break; fi
  sleep 1
done
[ -n "$URL" ] || fail "远程访问没有上线（状态 $STATE）"
echo "远程地址：$URL"
"$BIN" info | grep 隧道 || true

# 2. 未配对：可以打开页面，但拿不到任何终端
ok=
for _ in $(seq 1 30); do
  if curl -fsS -m 10 "$URL/api/me" > "$WORK/me.json" 2>/dev/null; then ok=1; break; fi
  sleep 2   # 新的快速隧道地址需要几秒才能解析
done
[ -n "$ok" ] || fail "无法经 $URL 访问"
grep -q '"paired":false' "$WORK/me.json" || fail "未配对设备不应显示为已配对：$(cat "$WORK/me.json")"
curl -fsS -m 10 "$URL/" | grep -q '<div id="app"' || fail "手机页面没有加载"
WS="-s -o /dev/null -w %{http_code} -m 5 --http1.1 -H Connection:Upgrade -H Upgrade:websocket -H Sec-WebSocket-Version:13 -H Sec-WebSocket-Key:dGhlIHNhbXBsZSBub25jZQ=="
CODE=$(curl $WS -H "Origin: $URL" "$URL/api/events" || true)
[ "$CODE" = 401 ] || fail "未配对时终端列表应返回 401，实际 $CODE"
CODE=$(curl -s -o /dev/null -w '%{http_code}' -m 10 -X POST -H "Origin: https://evil.example" -H 'content-type: application/json' -d '{"code":"AAAA-BBBB","name":"x"}' "$URL/api/pair")
[ "$CODE" = 403 ] || fail "跨站请求应被拒绝，实际 $CODE"
CODE=$(curl -s -o /dev/null -w '%{http_code}' -m 10 "$URL/api/settings")
[ "$CODE" = 404 ] || fail "远程端口不应提供管理接口，实际 $CODE"

# 3. 配对：错误的配对码被拒绝，正确的配对码换到设备凭证
CODE=$(curl -s -o /dev/null -w '%{http_code}' -m 10 -X POST -H "Origin: $URL" -H 'content-type: application/json' -d '{"code":"AAAA-BBBB","name":"x"}' "$URL/api/pair")
[ "$CODE" = 400 ] || fail "没有进行中的配对时应返回 400，实际 $CODE"
PAIR=$(local_api -X POST "$L/remote/pair" | json "d['code']")
curl -fsS -m 10 -c "$WORK/jar" -X POST -H "Origin: $URL" -H 'content-type: application/json' \
  -d "{\"code\":\"$PAIR\",\"name\":\"smoke\"}" "$URL/api/pair" > /dev/null || fail "配对失败"
curl -fsS -m 10 -b "$WORK/jar" "$URL/api/me" | grep -q '"unlocked":true' || fail "配对后应已解锁"
CODE=$(curl -s -o /dev/null -w '%{http_code}' -m 10 -X POST -H "Origin: $URL" -H 'content-type: application/json' -d "{\"code\":\"$PAIR\",\"name\":\"again\"}" "$URL/api/pair")
[ "$CODE" = 400 ] || fail "配对码只能使用一次，实际 $CODE"

# 4. 用设备凭证连接终端
COOKIE=$(awk '!/^#/ || /^#HttpOnly_/ {print $6"="$7}' "$WORK/jar" | tr '\n' ';')
CODE=$(curl $WS -H "Origin: $URL" -H "Cookie: $COOKIE" "$URL/api/attach?session=smoke" || true)
[ "$CODE" = 101 ] || fail "终端连接应升级为 WebSocket，实际 $CODE"
local_api "$L/remote" | json "[e['kind'] for e in d['audit']]" | grep -q attach || fail "访问记录里没有打开终端"

# 5. 移除设备后凭证立即失效
DEV=$(local_api "$L/remote" | json "d['devices'][0]['id']")
local_api -X DELETE "$L/remote/devices/$DEV" > /dev/null
curl -fsS -m 10 -b "$WORK/jar" "$URL/api/me" | grep -q '"paired":false' || fail "移除后设备凭证仍然有效"
echo "远程访问冒烟测试通过（$MODE）"
