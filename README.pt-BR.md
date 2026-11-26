# NixApps

> **Uma Loja de Aplicativos nativa, leve e declarativa para NixOS e Home Manager — construída em Rust, GTK4, Libadwaita e Relm4.**

[![Licença: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Desenvolvimento: Assistido por IA](https://img.shields.io/badge/Development-AI--Assisted%20%2B%20Human--in--the--Loop-purple.svg)](AI_TRANSPARENCY.md)
[![Stack: Rust + GTK4 + Relm4](https://img.shields.io/badge/Stack-Rust%20%2B%20GTK4%20%2F%20Libadwaita-orange.svg)](docs/ARCHITECTURE.md)
[![Status: v1.0.0 — Pronto](https://img.shields.io/badge/Status-v1.0.0%20Pronto-brightgreen.svg)](docs/ROADMAP.md)

[🇺🇸 English](README.md) | [🇧🇷 Português (Brasil)](README.pt-BR.md)

---

## 🤖 Transparência: Projeto Desenvolvido com Inteligência Artificial

O **NixApps** é construído de forma transparente usando um fluxo de **Engenharia Assistida por IA com Supervisão Humana (Human-in-the-Loop)**:

- A visão do produto, as decisões arquiteturais e os testes reais em NixOS são conduzidos e aprovados por mantenedores humanos.
- Agentes de Inteligência Artificial atuam na arquitetura, implementação em Rust/GTK4, testes automatizados e documentação.
- Todo o histórico de gerenciamento e decisões está aberto em [`docs/PROJECT_LOG.md`](docs/PROJECT_LOG.md) e [`docs/adr/`](docs/adr/README.md), e o arquivo [`AGENTS.md`](AGENTS.md) permite que qualquer pessoa da comunidade também evolua o projeto com ou sem agentes de IA.

Leia mais em **[`AI_TRANSPARENCY.md`](AI_TRANSPARENCY.md)**.

---

## ✨ Por que o NixApps existe?

Gerenciar pacotes de forma gráfica no NixOS costuma falhar quando ferramentas tentam editar diretamente arquivos `configuration.nix` escritos à mão (quebrando `let ... in`, flakes, overlays e comentários).

O **NixApps** adota uma abordagem limpa e segura:

1. **Módulo `.nix` Dedicado:** No primeiro acesso, você seleciona a pasta da sua configuração Nix e escolhe o alvo (**Home Manager** ou **NixOS Sistema**).
2. **Importação de Uma Linha:** Você adiciona apenas uma linha (`./modules/nixapps-managed.nix`) nos seus `imports`. O NixApps **nunca altera seus outros arquivos manuais**.
3. **Configurações Customizadas por Aplicativo:** Além de instalar/desinstalar pacotes, você pode adicionar opções próprias (`programs.*`, overrides ou snippets Nix do GitHub do app) com validação automática via `nix-instantiate --parse`.
4. **Fila de Mudanças + Diff + Apply & Rebuild:** Enfileire vários apps para instalar/remover, confira o diff exato do `.nix` e clique em **Apply & Rebuild** para aplicar tudo via Polkit (`pkexec`) com log ao vivo.
5. **Sem Scraping Frágil:** Usa dados oficiais dos canais NixOS (`packages.json.br`) indexados localmente em **SQLite FTS5** (~1 ms de busca offline em 144k+ pacotes), enriquecidos com ícones do Flathub, briefings cacheados do GitHub e dependências via `nix eval`.
6. **100% Nativo e Leve:** Construído em **Rust + GTK4 / Libadwaita (Relm4)** — sem Electron, sem WebView, consumo mínimo de RAM e integração nativa com Wayland.

---

## 🚀 Como Usar ou Instalar (Nix Flakes)

```bash
# Rodar direto via Flake
nix run github:asphzeera/NixApps

# Ou em modo de desenvolvimento local
git clone https://github.com/asphzeera/NixApps.git
cd NixApps
nix develop
cargo test --workspace
cargo run -p nixapps-gui
```

---

## 📚 Documentação do Projeto

- [`docs/PRD.md`](docs/PRD.md) — Requisitos e escopo do produto
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — Arquitetura técnica e formato do módulo `.nix`
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — Fases de entrega (Fase 0 até v1.0)
- [`docs/PROJECT_LOG.md`](docs/PROJECT_LOG.md) — Histórico de gerenciamento do projeto
- [`docs/adr/`](docs/adr/README.md) — Registros de Decisões de Arquitetura (ADRs)
- [`AI_TRANSPARENCY.md`](AI_TRANSPARENCY.md) — Política de transparência sobre uso de IA
- [`AGENTS.md`](AGENTS.md) — Guia para agentes de IA contribuidores
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — Guia de contribuição

---

## 📄 Licença

Distribuído sob a **Licença MIT**. Veja [`LICENSE`](LICENSE).
