# Общее подключение

[English](../integration-guide.md) | [Русский](integration-guide.md)

Это руководство описывает исходный сценарий операций с PoW. Для входа на сайт,
проверки аккаунтов, CORS и картинок используйте [подключение сайта](website-integration.md).

## Обязанности сервера приложения

1. Примените собственные ограничения запросов и бизнес-операций.
2. Создайте криптографически случайный ID операции и сохраните неизменяемую
   привязку к действию и параметрам приложения.
3. Вызовите внутренний endpoint создания задачи с серверным секретом.
4. Передайте задачу и ID операции клиенту.
5. Клиент решает PoW и отправляет решение в публичный API.
6. Получив ID операции и токен на сервере приложения, загрузите сохранённую
   операцию. Не доверяйте действию и параметрам, присланным клиентом.
7. Сохраните ID запроса использования и используйте токен через внутренний API.
8. Сохраните факт проверки и выполните операцию однократно с помощью собственной
   транзакции или другого механизма идемпотентности.

При обрыве соединения во время использования токена повторяйте тот же
`request_id`. Использование в Gateveriq и транзакция приложения не образуют
распределённую транзакцию. Восстановление возможно в пределах срока квитанции.
Позже при неизвестном результате потребуйте новую проверку. Собственные записи
о проверке храните столько, сколько нужно приложению.

Не передавайте секрет клиентам. Публичного ID интеграции недостаточно для
создания задачи. Ограничение доменов и CORS не являются аутентификацией.

## Браузерный виджет

Для исходного API разместите файлы и проксируйте публичный API под origin
приложения. Например, `/gateveriq/widget.js`, `/gateveriq/worker.js` и
`/gateveriq/public/v1/solutions`; префикс `/gateveriq` удаляется при пересылке.
У исходного endpoint нет CORS. В режиме сайта отправка решения на другой
origin поддерживается отдельными маршрутами.

```html
<div id="verification"></div>
<button id="cancel" type="button">Отмена</button>
<script type="module">
import {GateveriqWidget} from '/gateveriq/widget.js';
const widget = new GateveriqWidget(document.querySelector('#verification'));
document.querySelector('#cancel').onclick = () => widget.cancel();
// Это endpoint вашего приложения: он обращается во внутренний API Gateveriq.
const prepared = await fetch('/api/operation/prepare', {method:'POST'}).then(r => {
  if (!r.ok) throw Error('Не удалось подготовить проверку');
  return r.json();
});
const token = await widget.verify(prepared.challenge, {
  solutionURL:'/gateveriq/public/v1/solutions'
});
// Допуск к операции определяет сервер приложения после использования токена.
await fetch('/api/operation/complete', {
  method:'POST', headers:{'Content-Type':'application/json'},
  body:JSON.stringify({operation_id:prepared.operation_id, verification_token:token})
});
</script>
```

Gateveriq не реализует `/api/operation/*`: это пример endpoints приложения.
Обрабатывайте исключения в интерфейсе и предлагайте новую попытку.
Метод `verify` также принимает `AbortSignal`.

Разрешите модули, Workers и API в Content Security Policy. Используйте HTTPS;
localhost подходит для теста. Worker расходует CPU и заряд: измеряйте время
решения на целевых телефонах, прежде чем включать проверку публично.

## Нативные клиенты

Запускайте `core::solve` в фоновой задаче. Пример CLI:

```sh
cargo run --locked -- solve --challenge challenge.json --submit-url https://example.com/gateveriq/public/v1/solutions
```

Без `--submit-url` команда выводит JSON решения. MVP предоставляет Rust-модуль
и CLI, но не установщики и не отдельные Android/iOS SDK. При отмене операции
клиент должен отбрасывать результат. Нативный решатель проверяет срок действия,
но отдельного механизма отмены у него нет.

Метод consume `verification_method` сохраняется при выдаче токена: `pow` или
`pow_visual`. Политики и обновление БД описаны в [интеграции сайта](website-integration.md).
Пример backend: [registration.mjs](../../examples/registration.mjs).
Проверка примера: `node tests/registration.mjs`.
