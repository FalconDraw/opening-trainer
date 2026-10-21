// Lists every position on every library line in index.html, one key per line ("board turn castling").
// usage: node targets.mjs index.html path/to/chess.js > targets.txt
import fs from 'fs'; import { createRequire } from 'module';
const [html, chessPath] = process.argv.slice(2);
const { Chess } = createRequire(import.meta.url)(chessPath);
const src = fs.readFileSync(html, 'utf8').split('<script>')[1].split('</script>')[0];
const LIBRARY = new Function(src + '; return LIBRARY;')();
const key = (fen) => fen.split(' ').slice(0, 3).join(' ');
const keys = new Set();
for (const o of LIBRARY) for (const l of o.lines) {
  const g = new Chess(); keys.add(key(g.fen()));
  for (const s of l[1].split(' ')) { if (!g.move(s)) break; keys.add(key(g.fen())); }
}
process.stdout.write([...keys].join('\n') + '\n');
console.error(`${LIBRARY.length} openings, ${keys.size} positions`);
