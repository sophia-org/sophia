# Thin foreign binding: imported C structs use the installed header's layout.
# No Sophia checkout, Rust runtime or handwritten C adapter is involved.
const header = "sophia_portal_requester_upload.h"
type
  Upload {.importc: "struct sophia_pqu", header: header, bycopy.} = object
  Operation {.importc: "struct sophia_pqu_operation", header: header, bycopy.} = object
    kind: cint
    fid: uint32
    offset: uint64
    bytes: ptr uint8
    length: csize_t
  Reply {.importc: "struct sophia_9p_reply", header: header, bycopy.} = object
    kind {.importc: "type".}: uint8
    count, error, iounit: uint32
    data: ptr uint8

proc init(u: ptr Upload; bytes: pointer; length: csize_t; maximum: uint32;
          now, deadline: uint64): cint {.importc: "sophia_pqu_init", header: header.}
proc operation(u: ptr Upload; now: uint64; msize: uint32;
               op: ptr Operation): cint {.importc: "sophia_pqu_operation", header: header.}
proc issued(u: ptr Upload; now: uint64; msize, fid: uint32): cint
  {.importc: "sophia_pqu_issued", header: header.}
proc reply(u: ptr Upload; r: ptr Reply; now: uint64): cint
  {.importc: "sophia_pqu_reply", header: header.}
proc held(u: ptr Upload): cint {.importc: "sophia_pqu_held", header: header.}
proc done(u: ptr Upload): cint {.importc: "sophia_pqu_done", header: header.}
proc release(u: ptr Upload; now: uint64): cint
  {.importc: "sophia_pqu_release", header: header.}

for length in [0, 1, 65536]:
  var payload: array[65536, uint8]
  for b in payload.mitems: b = 17
  var upload: Upload
  var op: Operation
  var qids: array[26, uint8]
  qids[0] = 128
  var r: Reply
  doAssert init(addr upload, addr payload[0], csize_t(length), 65536, 0, 0) != 0
  doAssert payload[0] == 17
  doAssert init(addr upload, addr payload[0], csize_t(length), 65536, 0, 100) == 0
  doAssert operation(addr upload, 1, 512, addr op) == 0 and op.kind == 1
  doAssert issued(addr upload, 1, 512, 4) == 0
  r.kind = 111
  r.count = 2
  r.data = addr qids[0]
  doAssert reply(addr upload, addr r, 1) == 0
  doAssert issued(addr upload, 1, 512, high(uint32)) == 0
  r.kind = 13
  r.iounit = 7
  doAssert reply(addr upload, addr r, 1) == 0
  var offset = 0
  while held(addr upload) == 0:
    doAssert operation(addr upload, 1, 512, addr op) == 0
    doAssert op.kind == 3 and op.fid == 4 and op.offset == uint64(offset)
    doAssert op.bytes == addr payload[offset]
    doAssert op.length > 0 and op.length <= 7
    let count = op.length
    doAssert issued(addr upload, 1, 512, high(uint32)) == 0
    doAssert operation(addr upload, 1, 512, addr op) == 0 and op.kind == 0
    r.kind = 119
    r.count = uint32(count)
    doAssert reply(addr upload, addr r, 1) == 0
    offset += int(count)
  doAssert offset == length and done(addr upload) == 0
  doAssert release(addr upload, 2) == 0
  doAssert operation(addr upload, 2, 512, addr op) == 0 and op.kind == 4
  doAssert issued(addr upload, 2, 512, high(uint32)) == 0
  r.kind = 121
  doAssert reply(addr upload, addr r, 2) == 0
  doAssert done(addr upload) != 0
  for b in payload: doAssert b == 17
