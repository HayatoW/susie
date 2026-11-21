# susie

susie は、 GPUI で作る Web ブラウザです。

今のアプリは、 Linux でフィクスチャのページを GPUI のウィンドウに表示します。画素は CPU でコピーします。ウィンドウ上のスクロールは、そのページへ届きます。起動すると、 `docs/evidence/metrics.jsonl` にその起動の `startup_ms` と `fps` を 1 行追加します。

- 目的と範囲は [ブリーフ](docs/brief.md) です。
- タスクは [ボード](docs/board.md) です。
- 決めたことは [ADR](docs/adr/) です。
- エージェントのルールは [AGENTS.md](AGENTS.md) です。
