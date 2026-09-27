import { describe, expect, it } from "vitest";

import { DecodePlanner, h264CodecString, readPacket } from "./packets";

// O pacote de config (SPS + PPS) da fixture real do núcleo
// (crates/spiegel-core/fixtures/screen-h264.bin, SM-G9600).
const CONFIG = Uint8Array.from([
  0x00, 0x00, 0x00, 0x01, 0x67, 0x42, 0x80, 0x1e, 0xda, 0x0f, 0x0f, 0x79, 0x79, 0x48, 0x28, 0x30, 0x30, 0x36, 0x85,
  0x09, 0xa8, 0x00, 0x00, 0x00, 0x01, 0x68, 0xce, 0x06, 0xe2,
]);

function binary(flags: number, pts: bigint, data: number[]): ArrayBuffer {
  const bytes = new Uint8Array(9 + data.length);
  bytes[0] = flags;
  new DataView(bytes.buffer).setBigUint64(1, pts);
  bytes.set(data, 9);
  return bytes.buffer;
}

describe("readPacket", () => {
  it("lê as flags, o PTS e o conteúdo do formato binário da casca", () => {
    expect(readPacket(binary(0b10, 8_842_464_219n, [0xaa, 0xbb]))).toEqual({
      config: false,
      keyFrame: true,
      pts: 8_842_464_219,
      data: Uint8Array.from([0xaa, 0xbb]),
    });
    expect(readPacket(binary(0b01, 0n, [1])).config).toBe(true);
  });
});

describe("h264CodecString", () => {
  it("monta o avc1 com o perfil, as restrições e o nível do SPS", () => {
    expect(h264CodecString(CONFIG)).toBe("avc1.42801e");
  });

  it("sem SPS não há codec", () => {
    expect(h264CodecString(Uint8Array.from([0, 0, 0, 1, 0x68, 0xce]))).toBeNull();
  });
});

describe("DecodePlanner", () => {
  const packet = (config: boolean, keyFrame: boolean, pts: number, data: number[] | Uint8Array) => ({
    config,
    keyFrame,
    pts,
    data: Uint8Array.from(data),
  });

  it("configura com o SPS e junta o config ao quadro-chave seguinte", () => {
    const planner = new DecodePlanner();
    planner.reset();

    expect(planner.plan(packet(true, false, 0, CONFIG))).toEqual({ kind: "configure", codec: "avc1.42801e" });
    const key = planner.plan(packet(false, true, 100, [0x00, 0x00, 0x00, 0x01, 0x65]));
    expect(key).toEqual({
      kind: "decode",
      type: "key",
      timestamp: 100,
      data: Uint8Array.from([...CONFIG, 0x00, 0x00, 0x00, 0x01, 0x65]),
    });
    expect(planner.plan(packet(false, false, 200, [0x41]))).toEqual({
      kind: "decode",
      type: "delta",
      timestamp: 200,
      data: Uint8Array.from([0x41]),
    });
  });

  it("descarta quadros até o primeiro quadro-chave", () => {
    const planner = new DecodePlanner();
    planner.reset();
    planner.plan(packet(true, false, 0, CONFIG));

    expect(planner.plan(packet(false, false, 100, [0x41]))).toEqual({ kind: "skip" });
    expect(planner.plan(packet(false, true, 200, [0x65])).kind).toBe("decode");
  });

  it("uma nova captura (rotação) exige config de novo", () => {
    const planner = new DecodePlanner();
    planner.reset();
    planner.plan(packet(true, false, 0, CONFIG));
    planner.plan(packet(false, true, 100, [0x65]));

    planner.reset();
    expect(planner.plan(packet(false, true, 200, [0x65]))).toEqual({ kind: "skip" });
    expect(planner.plan(packet(true, false, 0, CONFIG))).toEqual({ kind: "configure", codec: "avc1.42801e" });
  });
});
