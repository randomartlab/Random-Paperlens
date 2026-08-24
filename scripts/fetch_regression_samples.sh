#!/usr/bin/env bash
# M5.3 样本集获取：从 arXiv 下载多学科英文论文（≥10 篇）
# 1) 经典机器学习/统计论文（ID 已核验）  2) 按学科拉取最新论文（物理/生物/经济/数学）
# 输出到项目根 samples/ 目录，供 examples/regression.rs 使用
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIR="$ROOT/samples"
mkdir -p "$DIR"

fetch() {
  local id="$1"
  local out="$DIR/$id.pdf"
  if [ -s "$out" ] && head -c 5 "$out" 2>/dev/null | grep -q "%PDF"; then
    echo "跳过（已存在）: $id"
    return 0
  fi
  curl -sL --max-time 120 "https://export.arxiv.org/pdf/$id" -o "$out"
  if head -c 5 "$out" 2>/dev/null | grep -q "%PDF"; then
    echo "OK: $id"
  else
    echo "失败/非PDF: $id（已删除）"
    rm -f "$out"
  fi
}

# 经典论文（ID 确定有效）
IDS=(1706.03762 1409.1556 1512.03385 1207.0580 1603.05027 1703.06907 \
      1803.08823 1810.04805 1503.03585 2010.11929 2005.14165 2103.00020)
for id in "${IDS[@]}"; do fetch "$id"; done

# 按学科拉取最新论文（补齐多学科）
fetch_cat() {
  local cat="$1"
  local xml
  xml=$(curl -sL --max-time 60 \
    "http://export.arxiv.org/api/query?search_query=cat:$cat&start=0&max_results=2&sortBy=submittedDate&sortOrder=descending")
  local ids
  ids=$(echo "$xml" | grep -o 'http://arxiv.org/abs/[0-9.]*' | sed 's|.*/||' | head -2)
  for id in $ids; do fetch "$id"; done
}
fetch_cat "physics.ins-det"
fetch_cat "q-bio.NC"
fetch_cat "econ.GN"
fetch_cat "math.CA"

n=$(ls "$DIR"/*.pdf 2>/dev/null | wc -l | tr -d ' \n\r')
echo "样本数: $n（$DIR）"
if [ "${n:-0}" -lt 10 ]; then
  echo "警告: 样本数 <10，不满足 M5.3 要求（≥10 篇多学科英文 PDF）"
fi
