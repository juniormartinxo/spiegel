// SPIKE — throwaway. Serves probe.html, exposes a QPC-based clock, and prints the result
// the page posts back, then exits.  node serve.mjs [port]
import http from 'node:http';
import fs from 'node:fs';

const port = Number(process.argv[2] || 8765);
const page = fs.readFileSync(new URL('./probe.html', import.meta.url));

http.createServer((req, res) => {
  if (req.url.startsWith('/now')) {
    res.writeHead(200, {'content-type': 'application/json'});
    res.end(JSON.stringify({ms: Number(process.hrtime.bigint()) / 1e6}));
  } else if (req.url.startsWith('/result')) {
    let body = '';
    req.on('data', (c) => (body += c));
    req.on('end', () => {
      console.log(JSON.stringify(JSON.parse(body), null, 2));
      res.end('ok');
      setTimeout(() => process.exit(0), 100);
    });
  } else {
    res.writeHead(200, {'content-type': 'text/html'});
    res.end(page);
  }
}).listen(port, '127.0.0.1', () => console.log(`http://127.0.0.1:${port}/`));
