-- SRS 5.2: pg_trgm e pre-requisito do indice de trigrama (P-02), que fica
-- desativado por padrao. A extensao e criada agora para que habilitar a busca
-- parcial no futuro seja uma migration de uma linha.
CREATE EXTENSION IF NOT EXISTS pg_trgm;
