# A DLL da webcam do Windows 11 cria a memória compartilhada, e o Spiegel espera por ela

No Windows 11, a origem de mídia da Webcam virtual roda dentro do serviço Frame Server, na sessão 0. Por isso os quadros que ela lê do Spiegel precisam ficar numa seção de memória compartilhada entre sessões (`Global\`). O Windows só deixa processos com `SeCreateGlobalPrivilege` criarem esse tipo de seção, e um usuário comum não tem esse privilégio. Então a origem de mídia, que roda como LocalService e tem o privilégio, cria a seção com uma DACL explícita no momento em que um aplicativo abre a câmera. O Spiegel continua tentando abri-la (a cada 0,5 s, mais ou menos) e só começa a publicar quadros quando consegue.

Escolhemos isso porque ninguém vê a câmera antes de algum aplicativo abri-la. O custo é no máximo cerca de 0,5 s de imagem de espera no início, e em troca o Spiegel não precisa de nenhum componente extra. O protótipo (issue #1) funciona exatamente assim.

## Consequências

- O Spiegel não consegue publicar quadros antes de algum aplicativo abrir a Webcam virtual, então deve tratar "seção ainda não existe" como normal, não como erro.
- A origem de mídia não pode desfazer a seção enquanto o Spiegel a mantiver aberta. A seção existe enquanto qualquer um dos lados tiver um handle para ela.

## Opções consideradas

- **Um serviço do Windows que cria a seção na inicialização do sistema**: o Spiegel poderia publicar a qualquer momento, mas seria mais um componente para instalar, atualizar e manter fora da versão portável (Linux/macOS), sem nenhum benefício visível.
- **Dar `SeCreateGlobalPrivilege` aos usuários pelo instalador**: muda a política de segurança da máquina por um ganho marginal.
