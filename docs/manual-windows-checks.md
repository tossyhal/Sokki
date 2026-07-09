# Windows 手動確認

このファイルは、WSL だけでは証明できない v1 リリース候補の確認項目を記録する。Windows 向け成果物を生成した後、Windows 10/11 x64 実機で実行すること。

## 方針

- WSL では `pnpm build`、`pnpm typecheck`、Rust format、`cargo xwin check`、`cargo xwin clippy`、Windows-target test build までを確認できる。
- JobTracker の状態遷移、stop flush、worker event payload、frontend store 更新など、決定的な Rust / TypeScript ロジックは WSL で代替確認してよい。
- WSL の Windows-target test build はコンパイル確認であり、生成された `.exe` の実行挙動を証明しない。
- WSL の `tauri dev` や Linux 向け Tauri build を受け入れ証跡にしてはいけない。
- リリース前に、各項目へ確認した成果物、Windows バージョン、結果を記録する。
- 実装は v1 完成扱いとし、このファイルは Windows 実機依存の最終確認と再発防止メモとして管理する。

## WSL 代替確認

Windows 引き継ぎ前に WSL で実行できる確認:

- `pnpm build`: TypeScript/Vite production build。
- `pnpm typecheck`: TypeScript strict type checking。
- `cd src-tauri && cargo fmt --all --check`。
- `cd src-tauri && cargo xwin check --target x86_64-pc-windows-msvc`。
- `cd src-tauri && cargo xwin clippy --target x86_64-pc-windows-msvc --all-targets -- -D warnings`。
- `cd src-tauri && cargo xwin test --target x86_64-pc-windows-msvc --all-targets --no-run`: Windows-target test binary のコンパイル確認。
- `pnpm tauri:build:win`: CPU x64 NSIS installer の生成確認。

最新の WSL 生成成果物:

- `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/Sokki_0.0.0_x64-setup.exe`
- サイズ: 25,612,176 bytes
- 結果: 2026-07-09 に WSL2 クロスビルドで生成成功。
- 注意: これは package 生成の確認に限る。起動、インストール、WebView2、マイク、WASAPI loopback、mix capture、asset protocol 再生は Windows 10/11 x64 実機確認が必要。

## Windows 実機確認ログ

### 2026-07-09 Windows 11 Home / installed exe

確認対象:

- `C:\Users\PC_User\AppData\Local\Sokki\sokki.exe`
- app data: `C:\Users\PC_User\AppData\Roaming\com.sokki.app`
- 確認者: ユーザー

確認済み:

- exe 起動: OK。
- WebView2 表示: OK。真っ白画面にならず、Sokki UI が表示された。
- 単一インスタンス: OK。2回目起動で新規ウィンドウは増えず、既存ウィンドウが前面化した。
- Onboarding / モデルDL: OK。`medium-q5_0` 推奨、DL進捗、DL完了後の Settings / Record 選択を確認。
- サウンドチェック: OK。録音開始との排他、完了後のテストWAV再生を確認。
- マイク録音: OK。録音WAV再生も確認。
- システム音声録音: OK。録音WAV再生も確認。
- マイク + システム音声 mix 録音: OK。録音WAV再生も確認。

再確認対象:

- Whisper job が catalog の `fileName` ではなく model name をファイル名として渡していたため、`models\medium-q5_0` のような存在しないパスで context 作成していた。修正後の成果物で realtime/batch/retranscribe/import/export を再確認する。
- 音声レベルメーターは RMS の線形値をそのまま表示していたため、実用音量でも一桁に見えやすかった。表示を0-100の圧縮メーターに変更済み。backendの録音ゲインや保存音声は変更していない。
- CPU版 `medium-q5_0` では文字起こしが遅く感じる場合がある。realtime 強制確定は5秒へ短縮済み。速度優先の場合は `small` / `base` モデルも確認する。
- 音声再生速度は0.5x〜3.0xを0.05刻みで変更できるスライダーとして確認する。
- SessionDetail のセッションエラー説明と詳細メッセージは1箇所に集約済み。音声プレイヤー直前の重複表示がないことを確認する。

stop/status 変更について WSL テストで確認すべきこと:

- `stop_recording` はアクティブな realtime segment を flush し、worker 完了を待たずに返る。
- pending realtime job が残っている間、返却セッションは `transcribing` になり、pending がなければ `done` になる。
- JobTracker は `error` 状態を `done` で上書きしない。
- `session://status` は session store を通じて一覧/詳細 UI 状態を更新する。

## エクスポートダイアログ

前提: transcript segment を持つセッションが1件以上ある。

- セッション詳細を開き、segment がない場合は `エクスポート` ボタンが無効、segment がある場合は有効であることを確認する。
- `エクスポート` をクリックし、`TXT`、`SRT`、`MD` を別々に選び、Windows native save dialog が開くことを確認する。
- 各形式を app data 外のユーザー指定フォルダへ保存する。
- エクスポートファイルが UTF-8(BOMなし) かつ LF 改行であることを確認する。
- TXT は segment text を改行連結していることを確認する。
- SRT は連番と `HH:MM:SS,mmm --> HH:MM:SS,mmm` の時刻形式を持ち、VLC 等の SRT 対応プレイヤーで読み込めることを確認する。
- MD はタイトル、作成日時、長さ、モデル、言語、タイムスタンプ付き本文を含むことを確認する。
- エクスポート後に `保存先を開く` をクリックし、Explorer が対象ファイルを表示することを確認する。
- native save dialog をキャンセルし、ファイルが作られず、ダイアログが引き続き利用可能であることを確認する。
- 書き込み不能な場所へのエクスポートを試し、クラッシュせずエラーが表示されることを確認する。

## モデル管理

前提: Hugging Face へのネットワーク接続があり、モデルDL/検証を確認できる。

- Settings を開き、モデル一覧に catalog entries が表示され、状態バッジ(未DL / DL済 / 未検証 / 手動配置・未検証 / 破損)が正しく出ることを確認する。
- モデルDLを開始し、`model://progress` により進捗バーとダウンロード済み byte 表示が更新されることを確認する。
- 進行中DLをキャンセルし、進捗表示が消え、UI が継続利用でき、再試行は先頭から始まることを確認する。
- モデルDLを完了し、アプリ再起動なしで状態が更新されることを確認する。
- DL済みモデルを削除し、行が未DL状態へ戻ることを確認する。
- app models directory にモデルファイルを手動配置し、検証をクリックして SHA-256 が一致した場合に録音で選択可能になることを確認する。
- 不一致または truncate したモデルファイルで検証し、状態が破損になり録音で選択不可になることを確認する。
- Settings の既定モデル選択は usable なモデルのみを有効化し、未検証 app-downloaded model は未検証ラベル付きで選択可能であることを確認する。
- Record ページで unusable model が無効化され、選択中モデルが usable でない場合に録音開始ボタンが無効であることを確認する。

## オンボーディング

前提: app data directory を初期化するか、`settings.json` の `onboardingDone` を `false` にする。

- アプリ起動時、Library / Record / Settings より先に `/onboarding` へリダイレクトされることを確認する。
- ようこそ、モデル選択、言語選択、完了の各ステップを進める。
- モデル選択ステップでは `medium-q5_0` が既定/推奨選択であることを確認する。
- モデルDLを開始し、progress bytes が更新されること、キャンセル後もステップが利用可能であることを確認する。
- モデルDLをスキップしても onboarding を完了でき、その後 Settings を開けることを確認する。
- usable model をDL済みの状態で onboarding を完了し、Library が開くことを確認する。
- アプリ再起動後、完了済み onboarding が再表示されないことを確認する。
- 再度 `onboardingDone=false` にし、`/record` や `/settings` へ直接遷移しても onboarding に戻されることを確認する。

## インポートダイアログ

前提: usable なローカル Whisper モデルが1つ以上ある。

- Library で `インポート` をクリックし、audio file filter 付きの Windows native open dialog が開くことを確認する。
- 有効な音声ファイルを1つ選択し、成功の import result row が表示され、Library の session list が更新されることを確認する。
- 複数ファイルを選び、その中に無効または未対応ファイルを含め、失敗した各ファイルがファイル名・error code・理由付きで表示されることを確認する。
- native open dialog をキャンセルし、import が開始されず、古い結果が残らないことを確認する。
- usable model をすべて削除または破損させ、`インポート` クリック時に壊れた import flow ではなく Settings への誘導が出ることを確認する。
- フロントエンドが save path や app-data recording path を組み立てず、選択したファイルパスだけを Rust の `import_files` command に渡すことを確認する。

## サウンドチェックと録音の排他

前提: usable なローカル Whisper モデルが1つ以上ある。

- サウンドチェックを開始し、完了前に録音開始を試す。
- サウンドチェック中は録音が始まらず、session を作成せずに `SOUND_CHECK_BUSY` 相当のエラーが出ることを確認する。
- サウンドチェック完了後、録音を開始できることを確認する。
- 録音中にサウンドチェックを開始しようとし、すでに録音中である旨のエラーで拒否されることを確認する。

## リアルタイム文字起こし

前提: usable なローカル Whisper モデルが1つ以上ある。

- マイク録音を開始し、10秒以上連続して発話する。
- Record ページのライブ文字起こしパネルに realtime segment が追加され、通常は最新 segment が見える位置まで自動スクロールし、手動で下端から離れると `最新へ` が表示されることを確認する。
- セッション詳細の transcript に、録音中/文字起こし中の live badge と pending row が表示されることを確認する。
- 最初の realtime transcript segment が5秒+推論時間以内に表示されることを確認する。
- 新しい realtime segment が playback controls や header actions を隠さずに表示されることを確認する。
- 5秒境界付近の segment で、600ms overlap 由来の重複テキストが出ないことを確認する。
- import/batch transcription の pending 中に録音を開始し、realtime recording が優先され、録音停止後に batch work が再開することを確認する。
- 録音WAV duration が録音時間と一致し、realtime transcript timestamp が録音 duration 内に収まることを確認する。
- realtime job が pending の状態で録音停止し、Whisper 完了を待たずに UI が即セッション詳細へ移動することを確認する。
- pending realtime job が終わるまで session status が `transcribing` で、完了後に `session://status` 経由で `done` へ変わることを確認する。
- 文字起こしエラーを発生または模擬し、session が `done` に変わらず `error` と可視メッセージを保つことを確認する。
- 長時間録音で、上限10分前警告が表示され、3時間上限で自動停止し、二重の active recording state が残らないことを確認する。

## デスクトップ・デバイス確認

- 生成された Windows `.exe` を起動する。
- 生成された Windows `.exe` を2回起動し、2回目は新しいウィンドウを開かず既存 main window を focus/restore することを確認する。
- NSIS installer からインストールして起動する。
- WebView2 が UI を読み込むことを確認する。
- マイク録音を確認する。
- WASAPI loopback によるシステム音声録音を確認する。
- マイク + システム音声の mix 録音を確認する。
- マイク録音中に選択中入力デバイスを抜く/無効化し、録音が自動停止し、WAV が再生可能で、session が `DEVICE_LOST` message 付きの `error` になることを確認する。
- システム音声または mix 録音中に選択中出力デバイスを無効化し、同じ `DEVICE_LOST` 自動停止挙動を確認する。
- サウンドチェックWAVと録音WAVが WebView asset protocol 経由で再生できることを確認する。
- app restart 後も `convertFileSrc` の再生 path が機能することを確認する。
- `transcribing` session 中にアプリを終了した場合、次回起動時に `interrupted` になり、再文字起こしできることを確認する。
- `error` / `interrupted` session が Library と SessionDetail で可視 badge/message を表示することを確認する。
- Library card と SessionDetail の両方から削除を試し、in-app confirmation dialog が開き、キャンセル時は session が残ることを確認する。
- Record setup view で、選択 source に必要な入力/出力デバイスがない場合に明確な警告が表示されることを確認する。
- Settings 画面で CPU build note が表示され、GPU 使用 mode choices が無効化され、GPU mode 変更後に backend label が更新されることを確認する。
- 画面遷移後に戻っても、realtime transcript segment が SessionDetail で一度だけ表示されることを確認する。
