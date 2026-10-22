#!/bin/sh
# tankmini-audio — hear the pedal's processed guitar in your headphones while
# YouTube (or anything else) keeps playing through the same output.
#
# The pedal already shows up as a USB audio interface; this script only tells
# PipeWire to mix its input into your current output. No fork of the app is
# needed, and the tuner screen uses the same capture source.
#
#   tools/tankmini-audio.sh headphones   # guitar + everything else in the laptop output
#   tools/tankmini-audio.sh pedal        # make the pedal the default output (phones in the pedal)
#   tools/tankmini-audio.sh status       # what PipeWire currently sees
#
# Ctrl+C stops the routing and leaves the rest of the audio untouched.

set -eu

# The pedal identifies itself as "Jieli Technology USB Composite Device".
MATCH="${TANKMINI_AUDIO_MATCH:-Jieli}"

pedal_source() {
    pactl list short sources \
        | awk -v p="$MATCH" '$2 ~ p && $2 !~ /\.monitor$/ { print $2; exit }'
}

pedal_sink() {
    pactl list short sinks | awk -v p="$MATCH" '$2 ~ p { print $2; exit }'
}

usage() {
    cat <<'EOF'
uso: tankmini-audio.sh <comando>

  headphones   mistura a guitarra no fone/monitor atual (o resto do áudio continua)
  pedal        torna a pedaleira a saída padrão (plugue o fone nela)
  off          volta a saída padrão para a placa do notebook
  status       mostra os dispositivos de áudio da pedaleira
EOF
}

case "${1:-headphones}" in
    headphones)
        src="$(pedal_source)"
        [ -n "$src" ] || { echo "pedaleira não encontrada no PipeWire (ela aparece como 'Jieli ...')" >&2; exit 1; }
        sink="$(pactl get-default-sink)"
        echo "guitarra: $src"
        echo "saindo em: $sink  (junto com o resto do áudio)"
        echo "Ctrl+C para parar"
        # 128 frames @ 44.1 kHz ≈ 2.9 ms de buffer: baixo o bastante para tocar em cima
        exec env PIPEWIRE_LATENCY=128/44100 pw-loopback \
            --name tankmini-monitor \
            --capture-props="node.target=$src node.passive=true" \
            --playback-props="node.target=$sink node.passive=true node.autoconnect=true"
        ;;
    pedal)
        sink="$(pedal_sink)"
        [ -n "$sink" ] || { echo "saída de áudio da pedaleira não encontrada" >&2; exit 1; }
        pactl set-default-sink "$sink"
        echo "saída padrão: $sink — plugue o fone na pedaleira"
        ;;
    off)
        sink="$(pactl list short sinks | awk -v p="$MATCH" '$2 !~ p { print $2; exit }')"
        pactl set-default-sink "$sink"
        echo "saída padrão: $sink"
        ;;
    status)
        echo "fontes da pedaleira:"; pactl list short sources | grep -i "$MATCH" || true
        echo "saídas da pedaleira:"; pactl list short sinks | grep -i "$MATCH" || true
        echo "saída padrão: $(pactl get-default-sink)"
        ;;
    -h|--help|help)
        usage
        ;;
    *)
        usage
        exit 2
        ;;
esac
