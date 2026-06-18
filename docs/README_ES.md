# Recorrido de Código: Funcionamiento del Emulador de Game Boy

¡Bienvenido al recorrido técnico del emulador de Game Boy! Este documento detalla la arquitectura del emulador, explicando el flujo de ejecución del código, el mapa de memoria y el motor de renderizado de gráficos y sprites.

A lo largo de esta guía, utilizaremos diagramas, ejemplos de código reales del proyecto y analogías para ilustrar cómo cooperan los diferentes módulos de hardware emulados en Rust.

---

## 1. Vista General de la Arquitectura y Sincronización

El emulador está dividido en dos partes principales:
1. **El Núcleo (Core - `core-gb`):** Que contiene la emulación pura del hardware (CPU, Bus de Memoria, PPU, APU, Cartucho).
2. **El Frontend (Desktop - `frontend-desktop`):** Que maneja la ventana gráfica, la entrada del usuario, la reproducción de audio y el bucle de renderizado a 60 FPS mediante la biblioteca `minifb`.

A continuación se muestra el diagrama de arquitectura del emulador, que ilustra cómo se interconectan los componentes a través del Bus de Memoria y cómo se sincronizan el CPU y el PPU:

![Arquitectura del Emulador](architecture_diagram.png)

### Sincronización Basada en Ciclos (Cycle-Accurate Emulation)

Una de las dificultades más grandes al emular una consola clásica como el Game Boy es mantener la sincronización temporal entre la unidad de procesamiento central (CPU) y la unidad de procesamiento de imágenes (PPU). En la consola real, ambos chips funcionan en paralelo a frecuencias fijas (CPU a ~4.19 MHz).

Para lograr esto de forma eficiente y precisa en Rust, el emulador utiliza un diseño **sincronizado por pasos**:
1. El CPU ejecuta una instrucción llamando a `step()` en [cpu.rs](file:///C:/Users/kanib/source/repos/gb-gba-emulator/core-gb/src/cpu.rs#L519).
2. Esta ejecución devuelve un `StepResult` que contiene el número de ciclos de reloj (M-cycles) que tardó en completarse esa instrucción (normalmente entre 4 y 24 ciclos).
3. Inmediatamente después, el emulador avanza el PPU por esa **misma cantidad de ciclos** llamando a `ppu.step()` en [ppu.rs](file:///C:/Users/kanib/source/repos/gb-gba-emulator/core-gb/src/ppu.rs#L152).
4. Este proceso se repite en un bucle dentro de `run_frame()` en [lib.rs](file:///C:/Users/kanib/source/repos/gb-gba-emulator/core-gb/src/lib.rs#L235) hasta que el PPU reporta que ha completado el renderizado de un fotograma entero (154 líneas de exploración completadas).

> [!NOTE]
> **Analogía del Narrador y el Proyector de Cine:**  
> Imagina que el **CPU** es un narrador que lee un guion (las instrucciones del juego). Cada frase que lee le lleva un tiempo diferente (ciclos). El **PPU** es un proyector de cine que dibuja la película en la pantalla. Para evitar que el narrador hable de una escena que la pantalla aún no muestra, cada vez que el narrador termina de leer una frase de 10 segundos, obligamos al proyector a avanzar exactamente 10 segundos de película. Así, la imagen y el flujo del código van siempre de la mano de forma perfecta.

---

## 2. Motor de Ejecución de Código: CPU y Bus de Memoria

### El CPU Sharp LR35902 (`cpu.rs`)

El CPU del Game Boy ([Cpu](file:///C:/Users/kanib/source/repos/gb-gba-emulator/core-gb/src/cpu.rs#L140)) es una variante híbrida entre el Intel 8080 y el Zilog Z80. Es un procesador de 8 bits con un espacio de direcciones de 16 bits.

#### Registro de Estado y Flags (`Registers`)
El CPU cuenta con varios registros representados en la estructura [Registers](file:///C:/Users/kanib/source/repos/gb-gba-emulator/core-gb/src/cpu.rs#L97):
- Registros individuales de 8 bits: `A`, `B`, `C`, `D`, `E`, `H`, `L` y `F` (registro de Flags).
- Pares de registros combinados de 16 bits para direccionamiento de memoria: `BC`, `DE` y `HL` (implementados en `cpu.rs` mediante métodos como `bc()`, `de()` y `hl()`).
- Registros de control de 16 bits: `SP` (Stack Pointer - Puntero de Pila) y `PC` (Program Counter - Contador de Programa).

El registro `F` almacena los indicadores de estado tras realizar operaciones aritméticas o lógicas:
- **Z (Zero Flag, Bit 7):** Se activa si el resultado es cero.
- **N (Subtract Flag, Bit 6):** Se activa si la última operación fue una resta.
- **H (Half-Carry Flag, Bit 5):** Se activa si hubo desbordamiento del bit 3 al 4 (acarreo de *nibble*).
- **C (Carry Flag, Bit 4):** Se activa si hubo desbordamiento del bit 7 al 8.

> [!NOTE]
> **¿Qué es un Nibble?**  
> Un *nibble* (o semiocteto) es una unidad de almacenamiento de información compuesta por **4 bits** (la mitad exacta de un byte de 8 bits). 
> Dado que 4 bits pueden representar 16 valores únicos (`0` a `15`), un nibble equivale exactamente a un solo dígito hexadecimal (ej. `0x0` a `0xF`). 
> En arquitecturas de 8 bits como la del Game Boy, los bytes se dividen lógicamente en un **nibble alto** (los 4 bits superiores) y un **nibble bajo** (los 4 bits inferiores). El acarreo auxiliar (*half-carry*) detecta cuándo una suma en el nibble bajo desborda e invade el nibble alto (es decir, cuando se genera una cifra a partir de la suma de los primeros 4 bits).

#### El Bucle Fetch-Decode-Execute
El método principal [Cpu::step](file:///C:/Users/kanib/source/repos/gb-gba-emulator/core-gb/src/cpu.rs#L519) simula la ejecución física del CPU:

1. **Fetch (Búsqueda):** Lee el código de operación (opcode) de 8 bits apuntado por el Program Counter (`PC`) a través del bus y avanza `PC` en 1.
   ```rust
   let opcode = self.fetch8(bus);
   ```
2. **Decode & Execute (Decodificación y Ejecución):** Utiliza un gran bloque `match` en Rust para identificar la instrucción correspondiente al byte leído y ejecutar la lógica asociada:
   ```rust
   let step_result = match opcode {
       0x00 => StepResult::new(4, false), // NOP: No hace nada, dura 4 ciclos.
       0x06 => {
           // LD B, n: Carga el siguiente byte en el registro B.
           self.registers.b = self.fetch8(bus);
           StepResult::new(8, false)
       }
       // ... otros 254 opcodes
       0xCB => {
           // Instrucciones de bits prefijadas con 0xCB
           let cb_opcode = self.fetch8(bus);
           self.execute_cb(cb_opcode, bus)
       }
   }
   ```
3. **Delayed Interrupt Enable:** Maneja la habilitación retardada de las interrupciones causada por la instrucción `EI` (Enable Interrupts).

#### Halt e Interrupciones
Cuando el juego ejecuta la instrucción `HALT`, el CPU entra en un modo de bajo consumo (`self.halted = true`) y deja de ejecutar instrucciones tradicionales. Solo se despierta cuando el Bus activa una interrupción pendiente.

El sistema de interrupciones ([Cpu::service_interrupt](file:///C:/Users/kanib/source/repos/gb-gba-emulator/core-gb/src/cpu.rs#L471)) maneja 5 fuentes de eventos asíncronos en orden de prioridad:
1. **VBlank (0x40):** Cuando el PPU termina de dibujar la pantalla visible.
2. **LCD STAT (0x48):** Cambios de estado del LCD (coincidencia de líneas de escaneo, etc.).
3. **Timer (0x50):** Desbordamiento del temporizador interno.
4. **Serial (0x58):** Transferencia de datos por cable de enlace (Link Cable).
5. **Joypad (0x60):** Entrada física de botones del mando.

Cuando ocurre una interrupción habilitada y las interrupciones globales (`IME`) están activas:
- Se desactiva el flag `IME`.
- Se almacena la dirección actual de `PC` en la pila (`push16`).
- Se salta a la dirección correspondiente al vector de la interrupción (ej. `0x0040` para VBlank).
- Esto consume 20 ciclos de reloj del procesador.

---

### El Bus de Memoria y la MMU (`bus.rs`)

La estructura [Bus](file:///C:/Users/kanib/source/repos/gb-gba-emulator/core-gb/src/bus.rs#L112) actúa como la unidad de gestión de memoria (MMU), mapeando el direccionamiento virtual de 16 bits del CPU (rango `0x0000` a `0xFFFF`) a los diferentes chips de hardware físicos:

| Rango de Dirección | Destino / Hardware Mapeado | Implementación en Código |
| :--- | :--- | :--- |
| `0x0000 - 0x7FFF` | Cartucho / ROM del Juego (Bancos variables) | [Cartridge::read_rom](file:///C:/Users/kanib/source/repos/gb-gba-emulator/core-gb/src/cartridge.rs) |
| `0x8000 - 0x9FFF` | Memoria de Video (VRAM) | `self.vram` (Bank-switchable en GBC) |
| `0xA000 - 0xBFFF` | Memoria Guardada del Cartucho (SRAM) | [Cartridge::read_ram](file:///C:/Users/kanib/source/repos/gb-gba-emulator/core-gb/src/cartridge.rs) |
| `0xC000 - 0xDFFF` | Memoria de Trabajo del Sistema (WRAM) | `self.wram` (Bancos del 0 al 7 en GBC) |
| `0xE000 - 0xFDFF` | Memoria de Eco (Espejo de `0xC000 - 0xDDFF`) | Redirige restando `0x2000` a la dirección |
| `0xFE00 - 0xFE9F` | Atributos de Sprites (OAM) | `self.oam` |
| `0xFEA0 - 0xFEFF` | Memoria no usable / Reservada | Devuelve `0xFF`, ignorado en escrituras |
| `0xFF00 - 0xFF7F` | Registros de Entrada/Salida (I/O) | `self.io` (Joypad, LCD, Temporizador, Serial) |
| `0xFF80 - 0xFFFE` | RAM de Alta Velocidad (HRAM) | `self.hram` |
| `0xFFFF` | Registro de Habilitación de Interrupciones (IE) | `self.ie` |

#### Lectura y Escritura en el Bus
Toda lectura de memoria se hace mediante `read8(address)` y toda escritura mediante `write8(address, value)`. 

El bus es el encargado de interceptar escrituras en registros especiales que desencadenan lógica compleja de hardware. Por ejemplo, el **OAM DMA (Direct Memory Access)** mapeado en el registro de I/O `0xFF46` ([bus.rs](file:///C:/Users/kanib/source/repos/gb-gba-emulator/core-gb/src/bus.rs#L649)):
```rust
if address == 0xFF46 {
    // Al escribir un valor X en 0xFF46, el sistema copia automáticamente
    // 160 bytes desde la dirección de origen (X * 0x100) directo a OAM.
    let source = u16::from(value) << 8;
    for offset in 0..OAM_SIZE {
        self.oam[offset] = self.read8(source + offset as u16);
    }
}
```

#### Los Temporizadores del Sistema (`tick_timer`)
El método [Bus::tick_timer](file:///C:/Users/kanib/source/repos/gb-gba-emulator/core-gb/src/bus.rs#L813) simula el temporizador interno del Game Boy:
- **El Registro DIV (`0xFF04`):** Incrementa cada 256 ciclos del CPU de forma constante. Escribir cualquier valor en este registro lo reinicia a `0`.
- **El Registro TIMA (`0xFF05`):** Incrementa a una frecuencia configurable por el registro de control `TAC`. Cuando `TIMA` se desborda superando `255`, se vuelve a cargar el valor almacenado en el registro modulo `TMA` (`0xFF06`) y se solicita una interrupción de temporizador (Bit 2 en `0xFF0F`).

---

## 3. Renderizado de Gráficos y Sprites: PPU y OAM

El Picture Processing Unit ([Ppu](file:///C:/Users/kanib/source/repos/gb-gba-emulator/core-gb/src/ppu.rs#L103)) es el motor gráfico de la consola. Rinde las imágenes línea por línea basándose en coordenadas y tiempos específicos del monitor CRT clásico.

### El Flujo de Scanlines y los Modos del LCD
La pantalla del Game Boy tiene una resolución física de 160x144 píxeles. Sin embargo, para simular los tiempos de retorno vertical de las antiguas pantallas de tubo, la PPU calcula **154 líneas de exploración (scanlines)** en total. Cada línea de exploración tarda exactamente **456 ciclos** en completarse, sumando un total de **70,224 ciclos por fotograma completo** (~59.73 Hz).

Durante el procesamiento de cada scanline visible (0-143), el PPU transiciona a través de tres modos representados en el registro `STAT` (`0xFF41`):

```text
|<------------- 456 ciclos de reloj de CPU por scanline ------------->|
+-------------------+----------------------------+--------------------+
| Modo 2: OAM Search| Modo 3: Pixel Transfer     | Modo 0: HBlank     |
| (80 ciclos)       | (172 ciclos aprox.)        | (204 ciclos aprox.)|
| Busca sprites     | Lee datos de VRAM y OAM    | Fin de línea,      |
| en la scanline    | y dibuja píxeles           | libre acceso CPU   |
+-------------------+----------------------------+--------------------+
```

- **Modo 2 (OAM Search):** Primeros 80 ciclos. El hardware analiza la memoria OAM para encontrar qué sprites coinciden verticalmente con la línea que se va a renderizar (máximo 10 sprites por línea).
- **Modo 3 (Pixel Transfer):** Siguientes 172 ciclos. El PPU lee los datos de azulejos (tiles) y atributos de sprites para alimentar al mezclador de píxeles y escribir en el framebuffer.
- **Modo 0 (HBlank):** Resto del ciclo (204 ciclos). El PPU entra en reposo horizontal. La memoria VRAM y OAM es totalmente accesible por el CPU.
- **Modo 1 (VBlank):** Ocurre de forma continua durante las líneas 144 a 153. Se activa la interrupción de VBlank, indicando al juego que puede actualizar la memoria gráfica sin interferir con lo que se dibuja en pantalla.

---

### Formato de Azulejos (Tiles) y Píxeles de 2bpp

Todos los gráficos en el Game Boy (tanto fondos como sprites) se componen de bloques de **8x8 píxeles** conocidos como **Tiles**. Cada píxel de un Tile tiene un color de 4 tonos posibles (índices 0, 1, 2, 3), codificados en un formato de **2 bits por píxel (2bpp)**.

En memoria, un Tile ocupa exactamente **16 bytes**. Cada fila de 8 píxeles del Tile se codifica utilizando **2 bytes consecutivos**:
- El primer byte almacena el bit menos significativo (LSB) de los 8 píxeles.
- El segundo byte almacena el bit más significativo (MSB).

#### Ejemplo de Decodificación 2bpp:
Supongamos que leemos los dos bytes correspondientes a una línea horizontal de un Tile:
- `Byte 1 (LSB): 0x5C` -> binario: `0 1 0 1 1 1 0 0`
- `Byte 2 (MSB): 0x3A` -> binario: `0 0 1 1 1 0 1 0`

Para calcular el índice de color de cada píxel de izquierda a derecha (bit 7 a 0):

```text
Píxel número:     0   1   2   3   4   5   6   7
-------------------------------------------------
Bit de Byte 2:    0   0   1   1   1   0   1   0   (MSB)
Bit de Byte 1:    0   1   0   1   1   1   0   0   (LSB)
-------------------------------------------------
Índice Resultón:  00  01  10  11  11  01  10  00
Índice Decimal:   0   1   2   3   3   1   2   0
```

> [!TIP]
> **Analogía del Tejido de Alfombras:**  
> Imagina que estás tejiendo una alfombra pixelada con hilos de colores. Para cada punto de la alfombra, miras dos hilos en paralelo: un hilo rojo (Byte 1) y un hilo azul (Byte 2). Si ambos hilos están ausentes, pintas el punto de blanco (0). Si solo hay hilo rojo, pintas de gris claro (1). Si solo hay hilo azul, pintas de gris oscuro (2). Si están ambos, pintas de negro (3). Combinando ambas capas obtienes el dibujo final.

Este proceso se implementa en [Ppu::render_frame](file:///C:/Users/kanib/source/repos/gb-gba-emulator/core-gb/src/ppu.rs#L281) para decodificar los píxeles:
```rust
let b1 = bus.read8(line_addr);         // Byte 1 (LSB)
let b2 = bus.read8(line_addr + 1);     // Byte 2 (MSB)

let bit = 7 - tile_col; // Dirección izquierda a derecha
let color_index = ((b2 >> bit) & 1) << 1 | ((b1 >> bit) & 1);
```

---

### Mapeo de Paletas DMG (Grayscale)
Los índices de color decodificados (0-3) no se dibujan directamente como tonos fijos. Pasan a través de un registro de paleta indexado:
- **BGP (`0xFF47`):** Paleta para el fondo (Background).
- **OBP0 / OBP1 (`0xFF48`/`0xFF49`):** Paletas para sprites.

Cada byte de paleta asigna a cada índice (0-3) un tono final de gris (0 = Blanco, 1 = Gris Claro, 2 = Gris Oscuro, 3 = Negro). Cada tono final ocupa 2 bits:
```rust
let shade = (palette >> (color_index * 2)) & 0x03;
```
Esto permite al juego hacer efectos como flashes de pantalla o desvanecimientos simplemente cambiando el registro de paleta en lugar de modificar todos los gráficos en la VRAM.

---

### Renderizado de Sprites (OAM Objects)

Los personajes y objetos móviles en pantalla son los **Sprites**. A diferencia del fondo estático, los sprites se leen de la memoria **OAM (Object Attribute Memory)**, que alberga hasta 40 sprites diferentes.

A continuación se detalla visualmente cómo funciona el renderizado de sprites en el PPU:

![Renderizado de Sprites](sprite_rendering.png)

#### Estructura de Atributos de Sprites en OAM
Cada sprite en OAM ocupa exactamente **4 bytes**:
1. **Byte 0: Coordenada Y.** Posición vertical del sprite en la pantalla. Se le resta `16` para posicionarlo (permite ocultar sprites parcialmente en la parte superior).
2. **Byte 1: Coordenada X.** Posición horizontal. Se le resta `8` (permite ocultarlo por los laterales).
3. **Byte 2: Índice del Tile.** Número de Tile en la memoria VRAM (rango `0x8000-0x8FFF`).
4. **Byte 3: Flags / Atributos.**
   - **Bit 7 (Priority):** Prioridad de dibujado (0 = Encima del fondo, 1 = Detrás del fondo si el fondo tiene color 1, 2 o 3).
   - **Bit 6 (Y-Flip):** Si es 1, dibuja el sprite invertido verticalmente.
   - **Bit 5 (X-Flip):** Si es 1, dibuja el sprite invertido de forma horizontal.
   - **Bit 4 (Palette Select):** Elige la paleta del sprite (0 = `OBP0`, 1 = `OBP1`).

#### Proceso Paso a Paso del Renderizado de Sprites
El algoritmo de renderizado de sprites en [Ppu::render_sprites](file:///C:/Users/kanib/source/repos/gb-gba-emulator/core-gb/src/ppu.rs#L382) funciona de la siguiente manera:

1. **Selección del tamaño:** Según el bit 2 de LCDC, los sprites pueden tener un tamaño de **8x8 píxeles** (usa 1 Tile) o **8x16 píxeles** (usa 2 Tiles contiguos).
2. **Bucle de Sprites:** Recorre secuencialmente los 40 sprites en OAM.
3. **Verificación de Posición:** Si la coordenada del sprite cae fuera de los límites de la pantalla, o si su fila actual no coincide con el escaneo, pasa al siguiente.
4. **Volteo de píxeles (Flip):**
   - Si `y_flip` es verdadero, invertimos las líneas de lectura vertical: `tile_y = sprite_height - 1 - sy`.
   - Si `x_flip` es verdadero, invertimos la lectura horizontal: `bit = sx`. Si es falso, leemos de izquierda a derecha: `bit = 7 - sx`.
5. **Transparencia:** Para los sprites, el **Índice de color 0 es siempre transparente**. Si el píxel decodificado del sprite tiene índice 0, no se dibuja y se conserva el fondo.
6. **Cálculo de Prioridad:**
   - Si el bit de prioridad del sprite es `0` (encima del fondo), el sprite siempre se dibuja sobre el fondo.
   - Si el bit de prioridad del sprite es `1` (detrás del fondo), solo se dibuja si el píxel de fondo actual tiene un índice de color `0` (color transparente del fondo).

```rust
// Lógica de prioridad de Sprites implementada en Rust:
if priority {
    // Detrás del fondo: solo dibuja si el fondo actual es de color 0
    let bg_color = self.framebuffer[pixel_idx];
    if bg_color == 0 {
        self.framebuffer[pixel_idx] = shade;
    }
} else {
    // Encima del fondo: siempre dibuja
    self.framebuffer[pixel_idx] = shade;
}
```

> [!TIP]
> **Analogía de los Recortes de Papel y el Vidrio:**  
> Imagina que el renderizado de la pantalla es un collage de recortes de papel. El fondo es un gran dibujo sobre cartulina blanca. Los sprites son recortes de personajes dibujados en papel transparente. 
> La memoria OAM actúa como las coordenadas de colocación. El "Color 0" es la parte transparente del recorte donde no pintaste nada. 
> La "Prioridad" determina si deslizas el recorte del personaje **por encima** de la cartulina (siempre visible) o **por debajo** de ella a través de unos agujeros precortados. Si la cartulina de fondo no está en blanco (color 0), el personaje que está por debajo quedará tapado por el fondo.

---

## 4. El Ciclo de Ejecución Completo: Integración de Componentes

Para concluir, veamos cómo interactúan todos los componentes explicados en una iteración del bucle principal de juego. Cada ciclo del juego (1 fotograma de 16.6ms) realiza una serie de pasos secuenciales que involucran al Frontend, el Núcleo (Core), el CPU, el Bus de datos y el PPU:

![Ciclo de Ejecución Completo](execution_flow.png)

Este proceso continuo a 60 fotogramas por segundo es lo que nos permite jugar de forma fluida y revivir la magia del hardware clásico del Game Boy en software moderno.
