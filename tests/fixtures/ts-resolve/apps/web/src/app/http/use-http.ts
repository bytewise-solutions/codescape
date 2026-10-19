import { http, Http, Mixed, Server } from './http';

export function load(h: Http, s: Server, m: Mixed) {
  h.get('/a');
  h.post('/b');
  const c = h.create('x');
  c.get('/c');
  http('/d');
  const r = http('/e');
  s.close();
  s.get('/');
  m.extra();
  m.get('/f');
}
