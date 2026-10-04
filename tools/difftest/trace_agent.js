// Frida agent for the diff test's runtime trace of the original Discord Times (trace.py).
//
// Runs inside the game (the Windows x86 frida-gadget). The harness sends hook specs with
// rpc.exports.install(); every call of a hooked function is buffered as one record and the
// harness collects them with rpc.exports.drain(). Nothing is sent unasked, so no record is
// lost or reordered.
//
// A spec: {name, addr, args: [[name, where, type]...], enter: {name: expr}, leave: {name: expr},
//          ret: type|null, when: expr, log_if: expr, max: int}
// - where: "eax" | "edx" | "ecx" | "stack:i" ([esp+4+4i] at entry; Delphi's register
//   convention passes the 1st-3rd arguments in EAX, EDX, ECX, the rest pushed left to right,
//   so the last argument is stack:0).
// - type: "i32" | "u32" | "i8" | "u8" | "i16" | "u16" | "hex".
// - expressions are JavaScript over: a (the arguments by name), e (the enter values),
//   l (the leave values), r (the return value), and the helpers i32 u32 i16 u16 i8 u8
//   (read memory), army(k) (army record address), t() (game time in centi-minutes).
// - when (at entry) and log_if (at leave) filter; max caps the records of that hook.
// - ret: null (or a spec without leave and ret) hooks the entry only; use it for a code
//   piece that is not a function start.

const TIME_CS = ptr(0x68dcb8);
let step = -1;
let seq = 0;
let buf = [];
let dropped = 0;
const LIMIT = 500000;
const hooks = [];

const H = {
  i32: (x) => ptr(x).readS32(),
  u32: (x) => ptr(x).readU32(),
  i16: (x) => ptr(x).readS16(),
  u16: (x) => ptr(x).readU16(),
  i8: (x) => ptr(x).readS8(),
  u8: (x) => ptr(x).readU8(),
  army: (k) => 0x75a940 + k * 0x3827,
  t: () => TIME_CS.readS32(),
};
const HN = Object.keys(H);

function compile(expr) {
  if (expr === undefined || expr === null) return null;
  const f = new Function('a', 'e', 'l', 'r', ...HN, 'return (' + expr + ');');
  const hv = HN.map((k) => H[k]);
  return (a, e, l, r) => f(a, e, l, r, ...hv);
}

function conv(v, type) {
  switch (type) {
    case 'u32': return v >>> 0;
    case 'i8': return (v << 24) >> 24;
    case 'u8': return v & 0xff;
    case 'i16': return (v << 16) >> 16;
    case 'u16': return v & 0xffff;
    case 'hex': return '0x' + (v >>> 0).toString(16);
    default: return v | 0;
  }
}

function readArg(ctx, where) {
  if (where === 'eax' || where === 'edx' || where === 'ecx' || where === 'ebx' ||
      where === 'esi' || where === 'edi') return ctx[where].toInt32();
  if (where.startsWith('stack:')) {
    const i = parseInt(where.slice(6), 10);
    return ctx.esp.add(4 + 4 * i).readS32();
  }
  throw new Error('bad argument place ' + where);
}

function evalMap(m, a, e, l, r) {
  if (!m) return undefined;
  const out = {};
  for (const k of Object.keys(m)) {
    try { out[k] = m[k](a, e, l, r); } catch (err) { out[k] = 'error: ' + err.message; }
  }
  return out;
}

function compileMap(m) {
  if (!m) return null;
  const out = {};
  for (const k of Object.keys(m)) out[k] = compile(m[k]);
  return out;
}

function push(rec) {
  if (buf.length >= LIMIT) { dropped++; return; }
  buf.push(rec);
}

function install(specs) {
  const done = [];
  for (const s of specs) {
    const h = {
      name: s.name, addr: ptr(s.addr), args: s.args || [], ret: s.ret === undefined ? null : s.ret,
      enter: compileMap(s.enter), leave: compileMap(s.leave), when: compile(s.when),
      logIf: compile(s.log_if), max: s.max || 0, count: 0,
    };
    const hasLeave = h.ret !== null || h.leave !== null || h.logIf !== null;
    const cb = {
      onEnter(ctx) {
        const a = {};
        for (const [n, where, type] of h.args) a[n] = conv(readArg(this.context, where), type);
        this.skip = (h.max && h.count >= h.max) || (h.when && !h.when(a, {}, {}, undefined));
        if (this.skip) return;
        const rec = { s: step, q: seq++, f: h.name, t: TIME_CS.readS32(),
                      ra: this.returnAddress.toUInt32(), a };
        rec.e = evalMap(h.enter, a, {}, {}, undefined);
        if (!hasLeave) { h.count++; push(rec); return; }
        this.rec = rec;
      },
    };
    if (hasLeave) {
      cb.onLeave = function (retval) {
        if (this.skip || !this.rec) return;
        const rec = this.rec;
        const r = h.ret ? conv(retval.toInt32(), h.ret) : undefined;
        rec.l = evalMap(h.leave, rec.a, rec.e || {}, {}, r);
        if (h.ret) rec.r = r;
        rec.tl = TIME_CS.readS32();
        if (h.logIf && !h.logIf(rec.a, rec.e || {}, rec.l || {}, r)) return;
        h.count++;
        push(rec);
      };
    }
    h.listener = Interceptor.attach(h.addr, cb);
    hooks.push(h);
    done.push(h.name);
  }
  Interceptor.flush();
  return done;
}

rpc.exports = {
  install(specs) { return install(specs); },
  setStep(n) { step = n; },
  drain() {
    const out = buf;
    const d = dropped;
    buf = [];
    dropped = 0;
    return { records: out, dropped: d };
  },
  detachAll() {
    for (const h of hooks) h.listener.detach();
    hooks.length = 0;
    Interceptor.flush();
  },
  info() {
    return { arch: Process.arch, platform: Process.platform, pid: Process.id,
             mz: ptr(0x400000).readU16(), hooks: hooks.map((h) => [h.name, h.count]) };
  },
};
