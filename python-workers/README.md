# ZooHelp Python Workers

Servicos auxiliares para IA e automacao. Nao devem virar API publica principal.

Responsabilidades:
- moderacao de imagens
- NLP e classificacao de conteudo
- recomendacoes avancadas
- analytics e dashboards internos
- modelos antifraude experimentais
- scripts admin

Rodar localmente:

```bash
python -m venv .venv
. .venv/Scripts/activate
pip install -r requirements.txt
uvicorn app.main:app --host 127.0.0.1 --port 8090
```

## Deploy em cloud

O worker deve ser publicado no mesmo projeto cloud do backend, mas como servico
separado da API Rust. O container da API atual executa apenas
`zoohelp-backend`; adicionar os arquivos Python ao repositorio nao inicia um
segundo processo nesse servico.

Configuracao recomendada para Railway:

- criar um novo servico a partir do mesmo repositorio GitHub;
- definir o root directory como `python-workers`;
- usar o `Dockerfile` desta pasta;
- gerar um dominio interno ou publico para o worker;
- configurar `AI_WORKER_URL` no servico Rust com a URL do worker.

Health check:

```text
/healthz
```
