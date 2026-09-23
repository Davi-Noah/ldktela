import { useState } from 'react';
import { media } from '../../app/runtime';
import type { CameraDevice } from '../../media/native';
import { useMediaStore } from '../../store/media';
import { Icon } from '../../ui/Icon';
import { IconButton } from '../../ui/IconButton';
import { MenuItem, MenuLabel, Popover } from '../../ui/Popover';

/**
 * Liga, desliga e troca a câmera (ADR-0038).
 *
 * Um controle próprio, e não um item dentro dos ajustes da transmissão: ligar a
 * câmera não exige estar compartilhando nada, e ela entra e sai muitas vezes
 * numa conversa. A escolha de dispositivo é que fica no menu, porque quase
 * ninguém tem duas câmeras e quem tem escolhe uma vez.
 *
 * Aparece nos dois lugares em que a sala pode estar: sobre o vídeo, na pílula de
 * controles, e no corpo da sala quando não há vídeo nenhum. Sem o segundo, quem
 * entrasse numa sala parada não teria como ligar a câmera — que é justamente
 * quando se quer.
 *
 * A lista é buscada ao **abrir** o menu: enumerar câmeras abre o Media
 * Foundation, e pagar isso em toda sala que se entra seria cobrar por um recurso
 * que a maioria não usa.
 */
export function CameraControl({ variant }: { variant: 'chrome' | 'body' }) {
  const camera = useMediaStore((state) => state.camera);
  const [devices, setDevices] = useState<CameraDevice[] | null>(null);

  const load = () => {
    void media
      .listCameras()
      .then(setDevices)
      .catch(() => {
        setDevices([]);
      });
  };

  if (camera.publishing) {
    // Desligar é a ação óbvia e fica num botão direto; trocar de câmera, que é
    // raro, fica no menu ao lado.
    return (
      <>
        <IconButton
          icon="camera"
          label={`Desligar a câmera${camera.deviceName === null ? '' : ` (${camera.deviceName})`}`}
          aria-pressed
          onClick={() => {
            void media.stopCamera();
          }}
        />
        <Popover
          icon="chevron"
          label="Escolher outra câmera"
          onOpen={load}
          align={variant === 'body' ? 'left' : 'right'}
        >
          {(close) => (
            <CameraList
              devices={devices}
              selected={camera.deviceId}
              onPick={(device) => {
                void media.switchCamera(device);
                close();
              }}
            />
          )}
        </Popover>
      </>
    );
  }

  return (
    <Popover
      icon="camera"
      label="Ligar a câmera"
      onOpen={load}
      disabled={camera.starting}
      align={variant === 'body' ? 'left' : 'right'}
    >
      {(close) => (
        <CameraList
          devices={devices}
          selected={null}
          onPick={(device) => {
            void media.startCamera(device);
            close();
          }}
        />
      )}
    </Popover>
  );
}

function CameraList({
  devices,
  selected,
  onPick,
}: {
  devices: CameraDevice[] | null;
  selected: string | null;
  onPick: (device: CameraDevice) => void;
}) {
  if (devices === null) {
    return <p className="px-2 py-1 text-text-muted">Procurando câmeras…</p>;
  }
  if (devices.length === 0) {
    // Dito por extenso: "nenhuma câmera" sem explicação manda a pessoa procurar
    // defeito no aplicativo, e o motivo costuma estar no Windows.
    return (
      <p className="px-2 py-1 text-text-muted">
        Nenhuma câmera encontrada. Verifique se ela está conectada e se o Windows permite o acesso.
      </p>
    );
  }
  return (
    <>
      <MenuLabel>Câmera</MenuLabel>
      {devices.map((device) => (
        <MenuItem
          key={device.id}
          selected={device.id === selected}
          onClick={() => {
            onPick(device);
          }}
        >
          <span className="flex items-center gap-2">
            <Icon name="camera" size={14} />
            {device.name}
          </span>
        </MenuItem>
      ))}
    </>
  );
}
