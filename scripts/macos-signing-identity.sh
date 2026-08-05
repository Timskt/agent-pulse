#!/usr/bin/env bash
#
# 造一张自签名的代码签名证书，让「辅助功能」授权不再每次构建就失效。
#
# 为什么需要它：macOS 把辅助功能授权挂在「指定要求」（designated requirement）上。
# 没有证书时 Tauri 只能临时签名（adhoc），指定要求就是**这一个二进制的哈希**：
#
#     designated => cdhash H"d4493a69ce70aca1e479fdcf225a852a2e74e91f"
#
# 改一行代码重新构建，哈希就变了，对系统来说这是另一个应用。于是系统设置里那个勾
# 还在（它记的是旧哈希）、实际已经不生效——这就是「我明明勾选了却还是敲不进去」。
#
# 用固定证书签过之后，指定要求变成认**名字加证书**：
#
#     designated => identifier "com.agentpulse.app" and certificate leaf H"…"
#
# 证书不换，这串就不变，勾一次一直有效。
#
# 跑一次就够了。之后每次构建：
#     export APPLE_SIGNING_IDENTITY="AgentPulse Self-Signed"
#     pnpm tauri:build
#
# 注意：自签名证书只解决「授权能不能留住」。它不是 Apple 开发者证书，
# 不能公证、别人下载仍会被 Gatekeeper 拦——分发得用真的 Developer ID。

set -euo pipefail

NAME="${1:-AgentPulse Self-Signed}"
KEYCHAIN="${HOME}/Library/Keychains/login.keychain-db"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "只有 macOS 需要这张证书。" >&2
  exit 1
fi

# 已经有了就别重复造：重复造会出现两张同名证书，codesign 反而会因为「身份不唯一」而失败
if security find-certificate -c "$NAME" "$KEYCHAIN" >/dev/null 2>&1; then
  echo "证书「${NAME}」已经存在。"
  echo
  echo "构建前设置："
  echo "    export APPLE_SIGNING_IDENTITY=\"${NAME}\""
  exit 0
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# extendedKeyUsage=codeSigning 是关键：少了它 codesign 不认这张证书。
# basicConstraints=CA:true 让它能自签（自己给自己背书）。
cat >"$WORK/openssl.cnf" <<'CNF'
[ req ]
distinguished_name = dn
x509_extensions    = ext
prompt             = no

[ dn ]
CN = PLACEHOLDER_CN

[ ext ]
basicConstraints = critical,CA:true
keyUsage         = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
CNF
# CN 走 sed 而不是直接插进 heredoc：证书名是外部输入，别让它有机会写进配置语法里
sed -i '' "s/PLACEHOLDER_CN/${NAME//\//_}/" "$WORK/openssl.cnf"

echo "① 生成密钥与证书…"
openssl req -x509 -newkey rsa:2048 -sha256 -days 3650 -nodes \
  -config "$WORK/openssl.cnf" \
  -keyout "$WORK/key.pem" -out "$WORK/cert.pem" 2>/dev/null

echo "② 导入登录钥匙串（可能要输一次密码或按 Touch ID）…"
# -T /usr/bin/codesign：只授权 codesign 用这把私钥，不用 -A 把权限敞给所有程序
security import "$WORK/key.pem" -k "$KEYCHAIN" -T /usr/bin/codesign
security import "$WORK/cert.pem" -k "$KEYCHAIN" -T /usr/bin/codesign

echo "③ 标记为可信（这一步会再问一次密码）…"
# 只写登录钥匙串的信任设置，不动系统钥匙串，所以不需要 sudo
security add-trusted-cert -r trustRoot -k "$KEYCHAIN" "$WORK/cert.pem" 2>/dev/null || {
  echo
  echo "自动设置信任没成功。手动补一下：打开「钥匙串访问」，在「登录」里找到" >&2
  echo "「${NAME}」，双击 › 信任 › 「代码签名」选「始终信任」。" >&2
}

echo
if security find-identity -v -p codesigning "$KEYCHAIN" | grep -q "$NAME"; then
  echo "✓ 好了。「${NAME}」已经是一个可用的签名身份。"
else
  echo "证书装上了，但还没被当成有效的签名身份——多半是信任那一步没生效。" >&2
  echo "照上面那段手动设置一下「代码签名 › 始终信任」。" >&2
fi

cat <<EOF

接下来：

    export APPLE_SIGNING_IDENTITY="${NAME}"
    pnpm tauri:build

装好新版本之后去「系统设置 › 隐私与安全性 › 辅助功能」把 AgentPulse
**取消再勾一次**（旧的那条记的还是哈希，得让它重新记一次名字）。
这是最后一次需要重勾——之后重新构建都不会再掉。

验证签名认的是名字而不是哈希：

    codesign -d --requirements - /Applications/AgentPulse.app

出现 \`certificate\` 字样就对了；只有 \`cdhash\` 说明还是临时签名。
EOF
