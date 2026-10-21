# Master-game statistics (masters.json)

`masters.json` holds, for every position on the trainer's library lines (plus up to 6 plies past
where games leave them), the moves masters played there, with wins/draws/losses and a few notable games.

Built by the **Build master stats** GitHub Action (`.github/workflows/masters.yml`):
it downloads the over-the-board PGN files of Lumbra's Giga Base from Hugging Face, keeps games where
both players are rated 2200+, and runs `mstats` (Rust) over them. Run it again from the Actions tab
(Run workflow) after adding openings to the library.

Data: Lumbra's Giga Base (https://lumbrasgigabase.com), CC BY-NC 4.0.
