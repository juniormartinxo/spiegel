// Os pacotes de vídeo que a casca envia em binário, e o que fazer com cada
// um no VideoDecoder. Sem dependência do WebCodecs, para ser testável.

export interface VideoPacket {
  /** Dados de configuração do codec (SPS e PPS no H.264), não um quadro. */
  config: boolean;
  keyFrame: boolean;
  /** Em microssegundos. 0 nos pacotes de config. */
  pts: number;
  /** O conteúdo codificado em Annex B, como o Dispositivo o produziu. */
  data: Uint8Array;
}

/** Lê o formato de `encode_packet` em `src-tauri/src/lib.rs`: 1 byte de
 *  flags (bit 0: config, bit 1: quadro-chave), o PTS em u64 big-endian e o
 *  conteúdo. */
export function readPacket(buffer: ArrayBuffer): VideoPacket {
  const view = new DataView(buffer);
  const flags = view.getUint8(0);
  return {
    config: (flags & 0b01) !== 0,
    keyFrame: (flags & 0b10) !== 0,
    pts: Number(view.getBigUint64(1)),
    data: new Uint8Array(buffer, 9),
  };
}

/** O codec no formato do WebCodecs (`avc1.PPCCLL`) a partir do SPS contido
 *  num pacote de config, ou `null` se não houver SPS. */
export function h264CodecString(config: Uint8Array): string | null {
  for (const nal of nalUnits(config)) {
    // nal_unit_type 7: SPS. Os 3 bytes seguintes são profile_idc,
    // as flags de restrição e level_idc.
    if ((config[nal] & 0x1f) === 7 && nal + 3 < config.length) {
      const hex = (byte: number) => byte.toString(16).padStart(2, "0");
      return `avc1.${hex(config[nal + 1])}${hex(config[nal + 2])}${hex(config[nal + 3])}`;
    }
  }
  return null;
}

/** Onde começa cada NAL (logo depois de cada código de início 00 00 01). */
function* nalUnits(data: Uint8Array): Generator<number> {
  for (let i = 0; i + 3 < data.length; i++) {
    if (data[i] === 0 && data[i + 1] === 0 && data[i + 2] === 1) yield i + 3;
  }
}

export type DecodeStep =
  | { kind: "configure"; codec: string }
  | { kind: "decode"; type: "key" | "delta"; timestamp: number; data: Uint8Array }
  | { kind: "skip" };

/** Decide o que fazer com cada pacote. O WebCodecs sem `description` espera
 *  Annex B com o SPS e o PPS dentro do quadro-chave, então o pacote de config
 *  é guardado e vai junto com o quadro-chave seguinte, como no scrcpy. */
export class DecodePlanner {
  private configured = false;
  private waitingKeyFrame = true;
  private pendingConfig: Uint8Array | null = null;

  /** Uma nova captura começou (início ou rotação): espera novo config e quadro-chave. */
  reset() {
    this.configured = false;
    this.waitingKeyFrame = true;
    this.pendingConfig = null;
  }

  plan(packet: VideoPacket): DecodeStep {
    if (packet.config) {
      const codec = h264CodecString(packet.data);
      if (!codec) return { kind: "skip" };
      this.configured = true;
      this.waitingKeyFrame = true;
      this.pendingConfig = packet.data.slice();
      return { kind: "configure", codec };
    }
    if (!this.configured || (this.waitingKeyFrame && !packet.keyFrame)) return { kind: "skip" };

    this.waitingKeyFrame = false;
    let data = packet.data;
    if (this.pendingConfig) {
      data = new Uint8Array(this.pendingConfig.length + packet.data.length);
      data.set(this.pendingConfig);
      data.set(packet.data, this.pendingConfig.length);
      this.pendingConfig = null;
    }
    return { kind: "decode", type: packet.keyFrame ? "key" : "delta", timestamp: packet.pts, data };
  }
}
