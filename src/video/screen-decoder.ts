// Decodifica o fluxo da Tela com o VideoDecoder do WebCodecs (por hardware
// quando disponível) e desenha cada quadro num canvas.
import type { VideoCodec } from "../core";

import { DecodePlanner, type VideoPacket } from "./packets";

export type DecoderProblem = { kind: "unsupportedCodec"; codec: string } | { kind: "failed"; detail: string };

interface Handlers {
  /** O primeiro quadro de cada captura foi desenhado. */
  onFirstFrame(): void;
  onProblem(problem: DecoderProblem): void;
}

export class ScreenDecoder {
  private readonly planner = new DecodePlanner();
  private readonly decoder: VideoDecoder;
  private readonly context: CanvasRenderingContext2D | null;
  /** Os pacotes passam em ordem por aqui, porque checar o suporte ao codec
   *  é assíncrono e nenhum quadro pode ser decodificado antes disso. */
  private queue: Promise<void> = Promise.resolve();
  private readonly canvas: HTMLCanvasElement;
  private readonly handlers: Handlers;
  private drewFirstFrame = false;
  private closed = false;

  constructor(canvas: HTMLCanvasElement, handlers: Handlers) {
    this.canvas = canvas;
    this.handlers = handlers;
    this.context = canvas.getContext("2d");
    this.decoder = new VideoDecoder({
      output: (frame) => this.draw(frame),
      error: (error) => this.fail({ kind: "failed", detail: error.message }),
    });
  }

  /** Uma nova captura (início ou rotação). */
  startCapture(codec: VideoCodec) {
    this.enqueue(async () => {
      if (codec !== "h264") {
        this.fail({ kind: "unsupportedCodec", codec });
        return;
      }
      this.planner.reset();
      this.drewFirstFrame = false;
    });
  }

  push(packet: VideoPacket) {
    this.enqueue(async () => {
      const step = this.planner.plan(packet);
      if (step.kind === "configure") {
        const config: VideoDecoderConfig = {
          codec: step.codec,
          optimizeForLatency: true,
          hardwareAcceleration: "no-preference",
        };
        const support = await VideoDecoder.isConfigSupported(config);
        if (this.closed) return;
        if (!support.supported) {
          this.fail({ kind: "unsupportedCodec", codec: step.codec });
          return;
        }
        this.decoder.configure(config);
      } else if (step.kind === "decode" && this.decoder.state === "configured") {
        this.decoder.decode(new EncodedVideoChunk({ type: step.type, timestamp: step.timestamp, data: step.data }));
      }
    });
  }

  close() {
    this.closed = true;
    if (this.decoder.state !== "closed") this.decoder.close();
  }

  private enqueue(task: () => Promise<void>) {
    // Uma exceção vira problema do decodificador em vez de travar a fila.
    this.queue = this.queue
      .then(() => (this.closed ? undefined : task()))
      .catch((error: unknown) => this.fail({ kind: "failed", detail: String(error) }));
  }

  private draw(frame: VideoFrame) {
    if (this.closed) {
      frame.close();
      return;
    }
    // O canvas tem o tamanho do vídeo; o CSS o ajusta à visualização
    // mantendo a proporção.
    if (this.canvas.width !== frame.displayWidth || this.canvas.height !== frame.displayHeight) {
      this.canvas.width = frame.displayWidth;
      this.canvas.height = frame.displayHeight;
    }
    this.context?.drawImage(frame, 0, 0);
    frame.close();
    if (!this.drewFirstFrame) {
      this.drewFirstFrame = true;
      this.handlers.onFirstFrame();
    }
  }

  private fail(problem: DecoderProblem) {
    if (this.closed) return;
    this.close();
    this.handlers.onProblem(problem);
  }
}
