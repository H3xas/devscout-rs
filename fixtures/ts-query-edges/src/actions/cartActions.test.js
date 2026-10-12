const { loadCart } = require('./cartActions');

it('loads', () => {
  expect(loadCart().type).toBe('load');
});
