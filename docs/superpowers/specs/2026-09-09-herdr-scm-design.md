# herdr-scm 設計文件

**日期**：2026-09-09
**狀態**：已核可，待產出實作計畫
**專案路徑**：`/home/bal/project/herdr-plugin2`
（設計文件原稿寫的是 `/home/leolu/projects/herdr-p`；該路徑在本機不存在，實作以本機實際目錄為準。）

## 1. 目標

在 herdr 內提供一個**唯讀的多 repo 版控總覽面板**，等同 VS Code Source Control 側欄的「Changes 樹 + diff」部分：一眼看完**當前 herdr workspace 底下所有 git repo** 的分支、領先/落後、以及每個變更檔案，並能直接檢視單檔 diff。

核心價值是 **herdr-file-viewer 沒有的那一件事：跨 repo 的總覽**。當多個 agent 同時在一棵樹的不同 repo 裡改東西時，這個面板是唯一能一眼掌握全局髒污狀態的地方。

## 2. 非目標（v1 明確排除）

- **任何寫入操作**：不 stage、不 commit、不 push/pull/sync、不 discard。全程唯讀。
- **Graph / commit log 區塊**：截圖下半部的 Outgoing Changes 圖，v1 不做（資訊密度低且與 `git log` 重複）。它是獨立區塊，日後要加不會動到既有結構。
- **跨 workspace 檢視**：只看當前 workspace，不聚合其他 space。
- **macOS / Windows 支援**：v1 只支援 Linux（開發環境為 WSL2）。manifest 保留 `platforms` 欄位，但不撰寫其他平台的 build script 與 `-windows` action。

## 3. 發現機制

### 3.1 掃描起點

```
HERDR_PLUGIN_CONTEXT_JSON { workspace_id, workspace_cwd, focused_pane_cwd }
  → herdr pane list（經 $HERDR_BIN_PATH）
  → 過濾 workspace_id 相符的 pane
  → 取其 cwd，去重
  → 每個 cwd 執行 git rev-parse --show-toplevel 上溯 repo root
     （不在 repo 內則以該 cwd 本身為掃描起點）
```

herdr CLI 不可用時，降級為「只用 `workspace_cwd` 當唯一掃描起點」。

### 3.2 走訪規則

從每個掃描起點遞迴走訪，尋找巢狀 `.git`，深度上限 `scan_depth`（預設 4）。

排除優先序，由高到低：

1. **`scan_excludes` 硬排除**（`node_modules`、`target`、`vendor`、`.venv`、`dist`）
   → 無條件剪枝，一層都不進去。
2. **`.gitignore` 命中的目錄 → 降級走訪，不剪枝**
   進入該目錄但**只找 `.git`**；找到就收下該 repo 並**停止再往下鑽**，找不到則走到 `scan_depth` 用盡後放棄。
3. 其餘目錄正常走訪。

> **規則 2 的理由（載重決策）**：本專案的目標 workspace 中，`teleagent/.gitignore` 含
> `/tenant-platform/`、`/teleagent-platform/`、`/pencil/`，而 5 個目標 repo 全部躺在這些被
> ignore 的目錄底下。若照一般作法遇 ignore 即剪枝，6 個 repo 會只剩 root 一個，功能歸零。
> 「被 ignore 但本身/底下是 repo 就留下」是本設計成立的前提，不是最佳化。

**走訪不因為找到 repo 而整體停止**：掃描起點本身即使是 repo，仍照常往下走訪（否則 `teleagent` 底下的 5 個 nested repo 一個都找不到）。唯一的停鑽點是規則 2 的降級走訪——在被 ignore 的子樹裡一旦命中 `.git` 就收下並停止再往下。其餘情況一律走到 `scan_depth` 用盡或被規則 1 剪掉為止；深度上限與硬排除清單是控制成本的手段，不靠「找到就停」。

submodule 由父 repo 的 `.gitmodules` 另行列舉，不依賴遞迴走訪找到。

### 3.3 repo 種類判定（純函式）

依 `.git` 的形態與父 repo 資訊判定，四選一：

| kind | 判定條件 |
|---|---|
| `root` | 該 repo 是掃描起點本身 |
| `submodule` | `.git` 是**檔案**，且路徑列於父 repo 的 `.gitmodules` |
| `worktree` | `.git` 是**檔案**，其 `gitdir:` 指向 `…/worktrees/…` |
| `nested` | `.git` 是**目錄**（獨立 clone） |

判定邏輯不做 I/O：輸入為「`.git` 是檔案或目錄」、「`gitdir:` 內容」、「父 repo 的 `.gitmodules` 路徑集合」，全部可單元測試。

### 3.4 重掃頻率

- **repo 清單重掃**（走檔案系統，較貴）：啟動時、按 `r` 時、每 `rescan_every`（預設 10）輪輪詢一次。
- **git 狀態快照**（`git status` 等）：每 `poll_interval_secs`（預設 3 秒）一輪。

## 4. 資料模型

```
RepoEntry {
  path: PathBuf,            // 絕對路徑
  display_name: String,     // 目錄名
  rel_path: String,         // 相對掃描起點；root 顯示為空
  kind: RepoKind,           // root | submodule | worktree | nested
  branch: Option<String>,   // detached HEAD 時為短 SHA
  ahead: u32, behind: u32,  // 無 upstream 時為 None
  groups: Vec<StatusGroup>,
  error: Option<String>,    // 該 repo 查詢失敗時的訊息
  stale: bool,              // 上一輪逾時
}

StatusGroup { kind: Staged | Changes | Untracked, files: Vec<FileEntry> }

FileEntry { path: String, status: char, orig_path: Option<String> }
```

`status` 取自 `git status --porcelain=v2 -z` 的 XY 欄位（`M`/`A`/`D`/`R`/`C`/`U`/`?`）。

### 4.1 diff 基準（依所屬分組決定）

選中的檔案位於哪一組，就決定用哪個基準產生 diff——與 VS Code 一致：

| 分組 | 指令 | 語意 |
|---|---|---|
| `Staged` | `git diff --cached -- <path>` | index vs HEAD |
| `Changes` | `git diff -- <path>` | 工作區 vs index |
| `Untracked` | 不呼叫 `git diff` | 直接讀檔內容，整份當作新增行呈現（受 §9 的大小上限與二進位偵測約束） |

同一個檔案同時出現在 `Staged` 與 `Changes` 時（XY 兩欄都非空），它會在兩組各出現一次，各自顯示自己基準的 diff。

## 5. UI 與互動

### 5.1 佈局（響應式）

單一 process、單一 herdr split pane，程式自繪兩個區塊（不做跨 pane IPC）。

- **pane 寬度 ≥ `split_threshold_cols`（預設 120）** → 左右分欄，左樹 40% / 右 diff 60%。
- **寬度 < 門檻** → 上下分割，樹在上、diff 在下，各佔滿整個寬度。

幾何計算是純函式 `(width, height) → Geometry`，可獨立測試。

```
寬 pane：
┌ SCM · herdr-p ── 6 repos · 3 dirty ─────────────────────────────────┐
│ ▾ teleagent    master ↑6↓1 root │ e2e/specs/07-authz-boundary.spec.ts│
│   ▾ Changes                  1  │ @@ -12,6 +12,9 @@                  │
│     U 07-authz-boundary.spec.ts │  test('B3 拒絕跨租戶', async () => {│
│ ▸ pencil       main    nested   │ +  await expect(page).toHaveURL(…) │
└─────────────────────────────────┴────────────────────────────────────┘

窄 pane：
┌ SCM · herdr-p ── 6 repos · 3 dirty ──────────┐
│ ▾ teleagent      master ↑6 ↓1  root       1  │
│   ▾ Changes                                  │
│     U  e2e/specs/07-authz-boundary.spec.ts   │
├─ 07-authz-boundary.spec.ts ──────────────────┤
│ @@ -12,6 +12,9 @@                            │
└──────────────────────────────────────────────┘
```

### 5.2 樹的三種列

1. **repo 列**：`名稱` `相對路徑`（root 省略）`branch` `↑ahead ↓behind` `kind 標記` `髒污檔數`
2. **分組列**：`Staged` / `Changes` / `Untracked`，該組為空則整列不顯示
3. **檔案列**：狀態字母 + repo 內相對路徑

### 5.3 按鍵

| 鍵 | 動作 |
|---|---|
| `j` `k` `↑` `↓` | 移動游標 |
| `Enter` `Space` | 展開／收合 repo 或分組 |
| `Tab` | 焦點切換 樹 ⇄ diff |
| `]` `[` | 跳至下／上一個變更檔（**跨 repo**） |
| `r` | 立即重掃（含 repo 清單） |
| `a` | 全部展開／收合 |
| `Z` | 全螢幕（`herdr pane zoom --current`） |
| `y` | 複製 `repo:path`（經 OSC 52 寫入終端剪貼簿，與 file-viewer 同機制） |
| `e` | 以 `$EDITOR` 開檔（純交棒，不寫檔） |
| `?` | 說明疊層 |
| `q` | 離開 |

所有動作經由鍵位註冊表定義，可由 config `[keys]` 覆蓋；`Esc` 永遠關閉疊層。

### 5.4 空狀態

當前 workspace 找不到任何 repo 時（例如 workspace cwd 是空目錄），顯示：找不到 repo 的說明、實際使用的掃描起點路徑清單、以及「按 `r` 重掃」提示。不得是一片空白。

### 5.5 狀態保存

每輪快照更新後重建樹時，以**列的身分**（repo 路徑 + 分組 + 檔案路徑）而非索引來還原展開狀態與游標位置，避免背景刷新造成游標亂跳。

## 6. 架構與模組

單一 process、library + 薄 binary（`main.rs` 只做 argv 解析與 `lib::run`），一切可測邏輯留在 library。

| 模組 | 責任 |
|---|---|
| `host` | 解析 `HERDR_PLUGIN_CONTEXT_JSON`；缺漏或格式錯誤退化為 `{ cwd }`，永不 panic |
| `herdr` | herdr CLI seam（`pane list`、`pane zoom`）；置於 trait 後，缺席時降級 |
| `discover` | 走訪與剪枝（§3.2），產出 `Vec<RepoRoot>` |
| `repo_kind` | 純函式種類判定（§3.3） |
| `git` | **唯一** shell out 到 `git` 的模組，僅唯讀子指令 |
| `model` | §4 的資料型別 |
| `tree` | 扁平列表、展開狀態、游標、跨 repo 跳轉、身分式狀態保存 |
| `poller` | 背景快照執行緒 |
| `render` | diff 文字產生、委派 `delta`、中和 ANSI escape |
| `layout` | 純函式響應式幾何（§5.1） |
| `presenter` | ratatui 繪製，回報幾何供滑鼠命中測試 |
| `input` / `intent` | 鍵位註冊表、key spec 解析、bindings 解析（config > default）、意圖 enum 與 dispatcher |
| `controller` | 意圖 → 狀態變更；收快照；派送 diff render job |
| `proc` | 共用的 `wait_bounded`（child wait + poll + timeout kill） |
| `config` | 唯讀 TOML 載入與退化 |
| `app` | event loop：draw → poll input → route → drain |

## 7. 資料流與並行

```
host::from_env → herdr::panes_in_workspace → discover::scan
   ↓
┌ poller thread ── 每 N 秒 → git::snapshot(repos) ──mpsc──┐
│                                                         ↓
│                                    controller.poll() → tree 重建
└ render worker ── diff job（委派 delta）──mpsc──→ presenter
```

- 兩條背景執行緒（`std::thread` + `mpsc`），**不使用 tokio**。
- 輸入執行緒永不阻塞於 git 或外部 renderer。
- diff job 帶單調遞增序號，使用者移開後抵達的舊結果直接丟棄。
- render worker 以 `catch_unwind` 包住，renderer panic 不會殺掉執行緒。

## 8. 設定

位置：`$HERDR_PLUGIN_CONFIG_DIR/config.toml`，其次 XDG fallback。唯讀，從不寫回。解析失敗**整份退化成預設值**，不 panic。

```toml
poll_interval_secs   = 3      # 0 = 關閉輪詢，退化成純手動 r
rescan_every         = 10     # 每 N 輪重掃 repo 清單
scan_depth           = 4
scan_excludes        = ["node_modules", "target", "vendor", ".venv", "dist"]
diff_tool            = "delta"   # "" = 純文字
split_threshold_cols = 120

[keys]
refresh = "r"
```

精度：`config > 環境變數 > 預設值`。

## 9. 錯誤處理與降級

**原則：單一 repo 的失敗絕不拖垮整個面板。**

| 情況 | 行為 |
|---|---|
| 某 repo 的 `git` 失敗／目錄消失 | 該列標 `!` 與錯誤訊息，其他 repo 照常刷新 |
| `git status` 逾時 | `proc::wait_bounded` timeout-kill，該列標 `stale`，下輪重試 |
| herdr CLI 不在或失敗 | 降級為只用 `workspace_cwd` 當唯一掃描起點 |
| `delta` 未安裝 | 純文字 diff + 一次性提示（非錯誤） |
| 走訪遇權限錯誤 | 略過該目錄，繼續 |
| 巨大 diff | 上限 2 MiB／5000 行，截斷並加尾註 |
| 二進位檔 | 顯示 `Binary file differs`，不呼叫 renderer |
| 找不到任何 repo | §5.4 空狀態畫面 |

## 10. 安全邊界

- 全程唯讀：不修改任何檔案，不修改任何 git 狀態。
- 只呼叫 `git` 的唯讀子指令：`status`、`rev-parse`、`rev-list`、`diff`、`worktree list`、`submodule status`。
- 外部 renderer 的輸出一律先中和 ANSI escape 再顯示。
- `e` 鍵是對 `$EDITOR` 的純交棒（spawn 後由編輯器接管終端），本程式不讀寫該檔。
- 所有狀態存在記憶體中，session 結束即消失。

## 11. 測試策略

採 TDD，先寫測試。

- **純函式單元測試（大宗）**：`repo_kind` 四種判定、`layout` 響應式幾何（含門檻邊界）、`tree` 扁平化／游標／跨 repo 跳轉／身分式狀態保存、`input` dispatcher 與 bindings 精度、`config` 解析與退化、`host` context 解析與退化、`discover` 的剪枝決策（以虛擬目錄結構輸入）。
- **git fixture 整合測試**：於 `tests/` 以 `git init` 現造一棵樹——root repo + 被 gitignore 的 nested clone + 真 submodule + `git worktree add`——驗證四種 `kind` 判定、§3.2 的剪枝規則（確認 5 個被 ignore 的 nested repo 都被找到），以及 `--porcelain=v2` 解析（含更名、未追蹤、衝突 `U`）。這是唯一碰真 git 的測試層。
- **seam 用 stub**：`herdr` CLI 與 `render` 均置於 trait 後，`controller` 全程以 stub 單元測試。

## 12. 封裝與安裝

`herdr-plugin.toml`：

- `id = "herdr-scm"`、`min_herdr_version = "0.9.0"`、`platforms = ["linux"]`
- `[[panes]]`：`id = "scm"`、`title = "SCM"`、`placement = "split"`、`command = ["./target/release/herdr-scm"]`
- `[[actions]]`：`open-scm`（split）與 `open-scm-tab`（獨立 tab，跨 tab 具幂等性）
- `[[build]]`：`["/bin/sh", "scripts/build.sh"]`，v1 直接 `cargo build --release`（不做 prebuilt 下載）

開發流程：`herdr plugin link /home/bal/project/herdr-plugin2`，改完 `cargo build --release` 即生效。使用者在 `~/.config/herdr/config.toml` 綁鍵位呼叫 `open-scm`。

## 13. 日後可加（不在 v1）

- Graph / commit log 區塊（截圖下半部）
- 寫入操作（stage / commit / push）
- prebuilt binary 的 fetch-or-build script 與 macOS / Windows 支援
- repo 清單的持久化快取
