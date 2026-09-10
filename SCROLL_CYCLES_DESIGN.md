# スクロール周回数指定の設計・実装引き継ぎ

状態: 実装・検証完了、コンシューマー公開前。2026-09-10。最低表示時間方式を反映した改訂版。

## 目的と採用方針

Slack Botなどが「画像を最低2周、かつ最低5秒スクロール表示する」と要求できるようにする。
秒数の推定ではなく、LEDサーバーが実際に描画したスクロール位置の周回数で終了を判断する。
既存の秒数指定も保持する。アイキャッチは周回数に含めない。

正常終了条件は completed_cycles >= scroll_cycles AND elapsed >= min_display_seconds。
周回数を満たしても最低時間に届くまではスクロールを継続する。
最低時間に達した際、周回数を満たしていれば途中の位置・周回でも終了する。
追加の周回完了は待たず、静止保持もしない。この条件を画像幅にかかわらず適用する。

初期リリースでは WORKER_TIMEOUT の既定値30秒と全体上限としての意味を維持する。
したがって2周は要求する終了条件であり、全体期限やシャットダウンより優先されない。
幅の大きい画像の2周や、全体期限の残りを超える最低表示時間を保証する仕様ではない。

## 調査時の基準

- led-service2: /Users/takanao/IdeaProjects/led-service2
  - HEAD: bf105631a865c04ef3bc52904b620f3c21dd794d
- slack-bot: /Users/takanao/IdeaProjects/slack-bot
  - HEAD: db78fc21adf5107eb25a0f45df16d4c6ba86e2fe
- API: led-service2/led-image-api サブモジュール。
  - 調査時、サーバー側APIは v0.1.4、Botのgo.modは v0.1.5。
  - 新APIは両者の既存変更を確認した共通の最新版から作る。古いサブモジュールだけを基に公開しない。

着手時には各リポジトリのAGENTS.md、HEAD、作業差分を再確認する。
この文書は実装方針を定義するもので、コミット・push・リリース・デプロイの実行指示ではない。

## API契約

SendImageRequest に次を追加する。既存フィールド番号1〜3を維持し、oneofへの移動はしない。

```proto
// Minimum number of horizontal scroll cycles. Zero uses duration_seconds.
// Positive values take precedence over duration_seconds and require scroll mode.
// Playback continues until both the cycles and min_display_seconds are satisfied.
// The worker timeout and shutdown may interrupt playback before completion.
uint32 scroll_cycles = 4;

// Minimum main display time in seconds, excluding eye-catch and preparation.
// Zero disables the minimum. Positive values require scroll_cycles > 0.
uint32 min_display_seconds = 5;
```

| 入力 | 新サーバーの動作 |
|---|---|
| scroll_cycles = 0、duration_seconds > 0 | 従来どおり秒数指定 |
| scroll_cycles = 0、duration_seconds <= 0 | INVALID_ARGUMENT |
| scroll_cycles > 0、duration_seconds >= 0 | 周回指定。秒数は新サーバーでは無視 |
| scroll_cycles > 0、duration_seconds < 0 | INVALID_ARGUMENT |
| scroll_cycles > 0、実効モードが静止画 | INVALID_ARGUMENT |
| scroll_cycles > 0、GIF | INVALID_ARGUMENT（明示SCROLLでも同様） |
| scroll_cycles > 0、未知のdisplay_mode値 | INVALID_ARGUMENT |
| scroll_cycles = 0、min_display_seconds > 0 | INVALID_ARGUMENT（秒数の値によらず拒否） |
| scroll_cycles > 0、min_display_seconds = 0 | 最低時間なし。指定周回数で終了 |
| scroll_cycles > 0、min_display_seconds > 0 | 周回数と最低時間の両方を満たして終了 |

上表の拒否条件を優先して適用する。min_display_secondsの未指定値はproto3既定の0。
両フィールドともuint32の範囲を受け付ける。最低時間がWORKER_TIMEOUTより長くても
受付時に拒否せず、全体期限で中断する。時間の型変換と計算はオーバーフローを防ぐ。

PPMのUNSPECIFIEDは既存規則に従いScrollとして受理する。
PNG/JPEGなどは明示SCROLLなら受理する。モード判定はworkerとserviceで共通化し、
MIME推論の既存挙動を不用意に変更しない。周回未指定時の未知モードの既存扱いは維持する。
検証のための画像デコードをgRPCハンドラーで行わない。

duration_seconds > 0 と scroll_cycles > 0 の併記を許可する理由は旧サーバー向けフォールバック。
旧サーバーは新フィールドを無視し、従来の秒数で表示する。
新サーバーではその秒数を周回表示の追加上限として使わない。
また、duration_secondsを最低時間として流用しない。旧サーバーでは最低時間の保証もない。
新旧で終了条件が異なることはREADMEに明記する。

scroll_cyclesの上限はprotoのuint32範囲とする。巨大値を指定しても全体期限で停止する。
周回数に比例する配列確保や画像複製は行わない。回数の積算にはオーバーフローしない設計を使う。

レスポンスは現状のsuccess/messageを維持する。成功はキュー受付を意味し、
表示完了・2周完走の保証ではない。完了通知RPC、進捗API、再試行方式の変更は今回の対象外。

## 1周の定義と描画アルゴリズム

Wはprepare後の画像幅。既存実装では高さをPANEL_ROWSに合わせ、整数除算で幅を決める。
元画像の横幅やPANEL_COLSでは数えない。既存のサイズ・メモリ制限を引き続き適用する。

1周は、offset=0からW-1までの各位置を順番に表示して、それぞれの位置の保持期間を終えること。
同じoffsetを複数回render_frameしても、複数ステップとして数えない。
最低時間がすでに満たされている場合、幅Wの2周の位置列は
0,1,...,W-1,0,1,...,W-1となり、3周目のoffset=0は描画しない。
最低時間が未達なら3周目以降も継続する。

実装手順の目安:

1. 全体Workの下でprepareと初期バッファ生成を行う。
2. offset=0、completed_cycles=0とし、表示準備後・初回描画直前を主画像の開始時刻とする。
   この時点から最低時間と位置の保持時間を計測する。
3. 全体期限・キャンセルを確認し、両終了条件を満たしていれば終了する。
   満たしていなければ現在のoffsetをrender_frameする。
4. SCROLL_INTERVAL_MSが経過するまでは同じ位置を再描画する。
5. 保持時間が経過したら次へ進む。offset=W-1ならcompleted_cyclesを加算する。
6. 描画後にも全体期限・キャンセルを確認し、両終了条件を満たしたら正常終了する。
   未達なら必要に応じてoffsetを進めてバッファを更新し、繰り返す。

最低時間の判定は位置遷移時だけでなく各描画の前後にも行う。
周回条件が達成済みなら、最低時間到達後の最初の判定で終了する。
全体期限・キャンセルと正常終了が同時に成立した場合は全体期限・キャンセルを優先する。
描画呼び出しは協調停止なので、終了精度は描画フレーム周期・呼び出しのブロック時間に依存する。

既存の1描画あたり最大1px進む挙動を維持する。遅延を取り戻すために位置を飛ばさない。
render_frameが失敗した位置は完了として数えない。
SCROLL_INTERVAL_MS=0は現状どおり1描画あたり1px進める。無描画で周回を消費しない。
W=1も1つの位置を保持する期間を1周として扱う。
W<PANEL_COLSでも既存の横方向反復描画を維持する。
画面外からの入場・退場、空白の挿入、短い画像を静止表示にする変更は含めない。

## 期限と終了結果

- 全体期限はdequeue + WORKER_TIMEOUT。キュー待機は含めない。
- アイキャッチ読込・表示、主画像デコード・準備も全体期限を消費する。
- 秒数指定は現状どおり準備後から計測し、全体期限との短い方で終了する。
- 周回指定ではduration_secondsの期限を作らず、全体Workの下で周回数と最低時間を判定する。
- 最低時間は下限条件であり、Workの打ち切り期限には設定しない。
  準備完了後からの単調時計の経過時間で比較する。
- アイキャッチは従来の秒数指定で再生し、主画像の周回数に影響しない。
- 全体期限が準備中に切れた場合は主画像の描画を開始しない。
- 終了時のclear、WindowClosedError伝播、キャンセル時の待機要求破棄を維持する。

内部モデルは例えば次のように、期限と表示終了条件を別の型で表現する。

```rust
enum DisplayLimit {
    Duration(Duration),
    ScrollCycles {
        cycles: std::num::NonZeroU32,
        min_display_duration: Duration,
    },
}
```

DisplayRequestには検証済みのDisplayLimitを保持する。
GIFとアイキャッチには時間指定だけを渡す。
共有ロジックはsrc/display/mod.rsに置き、ハードウェアbackendの変更を不要にする。

ログでは最低限、終了理由をduration_completed / cycles_completed / worker_timeout /
shutdown / errorとして区別する。全体期限到達を正常完走として記録しない。
cycles_completedは周回数と最低時間の両方を満たした正常終了だけを意味する。
主画像の開始・終了ログにrequested_cycles、completed_cycles、prepared_width_px、
min_display_seconds、scroll_interval_ms、elapsed_msを必要に応じて付ける。毎フレームのログは追加しない。
部分周回はcompleted_cyclesに切り上げない。統計の返し方は小さな結果型などで実装してよい。
最低時間待ちの追加周回も記録する。カウンターはu64の飽和加算などでラップを防ぐ。

例: W=20、30ms/px、2周、最低5秒なら、2周の目安1.2秒を超えてスクロールし、
約5秒で終了する（追加周回の途中でも終了）。W=300なら2周の目安18秒で終了する。

目安: W=300、30ms/px、2周なら最低約18秒。アイキャッチ5秒と準備時間が別途必要。
W=500なら目安30秒であり、全体上限30秒ではアイキャッチ込みで2周完走できない。
描画周期による遅れもあるため、見積もり値を完走保証や受付拒否の根拠にはしない。
WORKER_TIMEOUTの自動延長・新しい時間上限設定は今回追加しない。

## Rust CLI

- --scroll-cycles N を追加（1〜uint32最大値）。未指定なら従来の--duration動作。
- --min-display-seconds N を追加（0〜uint32最大値、既定値0）。
  正の値は--scroll-cyclesと併用必須。0は最低時間なし。
- --durationは既定値10秒を維持。周回数との併記を許可し、旧サーバー用秒数と説明する。
- --scroll-cycles指定時、display-mode未指定なら明示SCROLLを送信する。
- 明示staticまたはGIFと--scroll-cyclesの併用はエラーにする。
- RPC受付用のタイムアウトを推定周回時間に延長しない。
  既存CLIのduration+10秒という計算の全面改修は対象外。コメントは受付待ちであることを明確にする。

例:

```sh
cargo run --bin led-client -- --file banner.ppm --scroll-cycles 2 --min-display-seconds 5
cargo run --bin led-client -- --file image.png --scroll-cycles 2 --min-display-seconds 5 --duration 10
```

## Slack Bot

- SLACK_BOT_LED_SCROLL_CYCLESを追加する。既定値0（互換動作）、2で2周指定を有効化する。
- 環境変数は0〜uint32最大値を受け付ける。負数・不正文字・範囲外は警告し、既定値0に戻す。
- SLACK_BOT_LED_MIN_DISPLAY_SECONDSを追加（既定値0）。同じuint32のパース規則を使う。
  cycles=0で最低時間が正の場合は起動時に警告し、送信時は最低時間を0にする。
  これによりcyclesだけを0に戻して秒数方式へ戻せる。公開クライアントの新オプションでは
  cycles=0かつ最低時間が正の不正な組み合わせを送信前にエラーにする。
- SLACK_BOT_LED_IMAGE_DURATION_SECONDSは既定値10のまま維持する。
  周回指定時も正の値を送信し、旧サーバーではこの秒数にフォールバックする。
- Botは現在のPPM送信を継続する。周回指定時には明示SCROLLを送ると意図が明確になる。
- 公開クライアントSendImageの既存シグネチャを維持する。
  例えばSendImageWithOptionsとオプション型を追加し、共通の送信処理へ委譲する。
  既存メソッドはcycles=0、min_display_seconds=0で動くラッパーとする。
- config → bot初期化 → message handler → LED clientへ値を渡す。
- 画像幅から秒数を推定する処理やサーバーの速度設定をBotへ複製しない。
- LED_OPERATION_TIMEOUTはRPC受付の上限のまま維持する。

運用で有効化する値:

```ini
SLACK_BOT_LED_SCROLL_CYCLES=2
SLACK_BOT_LED_MIN_DISPLAY_SECONDS=5
SLACK_BOT_LED_IMAGE_DURATION_SECONDS=10
```

既定値0を採用するため、新Botの公開だけでは自動で周回指定に切り替わらない。
設定変更までが機能の有効化手順となる。Homelab側の変更は別途対象ファイルを確認する。

## 変更対象と生成物

1. led-image-api: proto、生成Go bindings、API説明。
   - buf.yaml / buf.gen.yamlを確認し、APIリポジトリのルートでbuf generateを実行する。
   - Go生成コードを手編集しない。buf lintと既存APIとの差分互換性チェックを行う。
   - 公開バージョンは実装時の最新タグを確認して決定する。
2. led-service2: APIサブモジュール参照、src/service.rs、src/worker.rs、src/display/mod.rs、
   src/bin/client.rs、必要なテスト支援・README。
   - Rust bindingsはbuild.rsのtonic_prost_buildで生成される。生成物を手編集しない。
   - 新フィールド追加に伴う全SendImageRequestリテラルを検索・更新する。
3. slack-bot: go.mod/go.sum、internal/config、internal/bot、internal/handlers、
   pkg/led/client、関連テスト、README。

## 必須の受入テスト

- API入力検証: 上記契約表を網羅。エラー要求はキューを消費しない。
- 互換性: scroll_cycles省略時は秒数方式。両方正なら周回優先。duration=0と正のcyclesも受理。
- 描画: 最低時間0で、幅3の識別可能な画像で2周のoffset列が0,1,2,0,1,2になる。
  同一位置のリフレッシュを除外して確認し、終端の余分な0がないことを検証する。
- 同一offsetの再描画で回数が増えない。描画が遅くても位置を飛ばさない。
- W=1、W<PANEL_COLS、画像の高さスケーリング後のWを使うケース。
- 周回条件が先に成立してもスクロールを継続し、最低時間到達時に追加周回の途中で終了する。
- 最低時間が先に成立しても指定周回数まで継続する。同時成立でも余分な描画をしない。
- 最低時間は初回描画直前から計測し、アイキャッチ・デコード・準備時間を含めない。
- 最低時間待ちの中断をcycles_completedとして記録しない。全体期限との同時成立は中断扱い。
- interval=0でも実際の描画を伴って最低時間まで継続し、周回カウンターがラップしない。
- 正常完了前の全体期限、キャンセル、描画エラーで完走扱いにしない。
- アイキャッチ完了後に主画像の周回数をゼロから開始し、期限はdequeue基準のまま。
- 準備時点で期限切れなら描画しない。既存のメモリ・画像制限テストを維持。
- CLIの組み合わせ、Bot設定パース、旧SendImageと新オプション送信のwire値。

タイミング依存を減らすため、位置遷移の小さな状態機械は経過時間を引数で受け取って
決定的にテストできる形が望ましい。全体を大規模な時計抽象へ書き換える必要はない。
FakeDisplayで描画内容と終了を検証し、GoのRPCサーバーgoroutine内でt.Fatalを呼ばない。

検証コマンド:

```sh
# led-service2
cargo fmt --all -- --check
cargo clippy --all-targets --no-default-features --features emulator
cargo test --no-default-features --features emulator

# slack-bot
go test ./...
# 既存CI/Makefileのformat/lintも確認して実行する。
```

CIのRPiビルドでbackendとのコンパイル互換性を確認する。ソース検証のために
実機へデプロイ・再起動・表示要求を行わない。実機の視覚確認はデプロイ後の別工程。

## リリースと移行

1. APIを生成・検証して公開する。
2. サーバーのサブモジュール参照を更新し、新サーバーを公開・デプロイする。
3. BotのAPI依存を更新し、新Botを公開・デプロイする。
4. Bot側SLACK_BOT_LED_SCROLL_CYCLES=2、SLACK_BOT_LED_MIN_DISPLAY_SECONDS=5を設定する。
5. 実運用でcycles_completedとworker_timeoutの比率を確認し、必要な場合にのみ
   WORKER_TIMEOUTを運用設定で調整する。

新Bot→旧サーバーは秒数フォールバック、新旧Bot→新サーバーは各指定に応じて動作する。
サーバーをロールバックすると完走回数は保証されなくなるが、受付不能にはならない。
Botのcyclesを0に戻すと既存の時間指定動作に戻せる。
リリース時はCargo.toml/Cargo.lockのversionとタグを一致させる。
Botのコンテナタグは既存CIではvなし（例: 0.2.2）。新しいタグ番号は未決定。

## 実装担当への依頼文

この文書に従い、実スクロール周回数による終了条件をled-image-api、led-service2、
slack-botに実装してください。最初に各AGENTS.mdと作業差分を確認し、既存の変更を保護してください。
設計上の主要決定は、フィールド番号4のscroll_cyclesと番号5のmin_display_secondsの追加、
指定周回数AND最低時間による終了、未達時のスクロール継続、全体期限維持、
旧サーバーへの正の秒数フォールバックです。Botの新設定の既定値はともに0、
運用例は2周かつ最低5秒です。画像幅が小さい場合も同じ終了規則を適用してください。
必要なbindingsを生成し、受入テストと各リポジトリの検証を実施してください。
コード変更と検証結果を報告し、公開・デプロイは別途の依頼があるまで実行しないでください。
