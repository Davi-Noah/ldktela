-- Câmera ao lado da tela (ADR-0038).
--
-- A unidade deixa de ser a pessoa e passa a ser a publicação, o par
-- (pessoa, fonte). Presença é onde mora "o que esta pessoa transmite agora",
-- então é aqui que a fonte aparece: uma coluna de início por fonte, nula quando
-- aquela fonte não está no ar.
--
-- Só adiciona. `share_sessions` fica como está de propósito: uma sessão cobre o
-- período em que a pessoa esteve ao vivo, independentemente de quantas fontes
-- usou. O relatório dela é o orçamento de egress (RNF-05), que soma bytes por
-- pessoa e não por trilha — e mantê-la intacta evita trocar o índice único, que
-- seria migration destrutiva sobre tabela em uso.
ALTER TABLE room_presence
    ADD COLUMN screen_since TIMESTAMPTZ,
    ADD COLUMN camera_since TIMESTAMPTZ;

-- `publishing` continua existindo e passa a significar "transmite alguma coisa".
-- É mantida na mesma instrução que escreve as colunas acima, nunca sozinha: ela
-- é atalho para a varredura de revogação e para a contagem de espectadores, e
-- não uma segunda verdade sobre qual fonte está no ar.
COMMENT ON COLUMN room_presence.publishing IS
    'Transmite alguma fonte. Derivada de screen_since/camera_since (ADR-0038).';
