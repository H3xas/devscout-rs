import { target } from './target';
import equal = require('./target');
const lazy = import('./target.js');
const unknown = import('./missing.js');
target();
equal.target();
void lazy;
void unknown;
