# susie

susie は、 GPUI で作る Web ブラウザです。

今のアプリは、 Linux で、起動時の引数の URL を GPUI のウィンドウに表示します。引数がないときは、フィクスチャのページを開きます。画素は CPU でコピーします。1 回の起動で、スクロールとクリックと、確定した文字をそのページへ届けます。その起動の `startup_ms` と `fps` は、 `docs/evidence/metrics.jsonl` に残ります。

- 目的と範囲は [ブリーフ](docs/brief.md) です。
- タスクは [ボード](docs/board.md) です。
- 決めたことは [ADR](docs/adr/) です。
- エージェントのルールは [AGENTS.md](AGENTS.md) です。
