use std::collections::VecDeque;

/// Buffer circular de líneas de log con capacidad fija.
///
/// Cuando se supera la capacidad, descarta la línea más antigua. Sirve
/// para conservar exactamente la "ventana de contexto" que se le envía
/// al agente en el momento del fallo.
#[derive(Debug, Default)]
pub struct RingBuffer {
    capacity: usize,
    lines: VecDeque<String>,
    bytes: usize,
}

impl RingBuffer {
    /// Crea un buffer vacío con la capacidad indicada.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            lines: VecDeque::with_capacity(capacity),
            bytes: 0,
        }
    }

    /// Inserta una nueva línea, descartando la más antigua si es necesario.
    pub fn push(&mut self, line: String) {
        if self.capacity == 0 {
            return;
        }
        if self.lines.len() >= self.capacity {
            if let Some(old) = self.lines.pop_front() {
                self.bytes -= old.len();
            }
        }
        self.bytes += line.len();
        self.lines.push_back(line);
        while self.bytes > 32 * 1024 {
            if let Some(old) = self.lines.pop_front() {
                self.bytes -= old.len();
            }
        }
    }

    /// Copia el contenido actual del buffer, en orden cronológico.
    pub fn snapshot(&self) -> Vec<String> {
        self.lines.iter().cloned().collect()
    }

    /// Cantidad de líneas actualmente almacenadas.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descarta_la_linea_mas_antigua_al_superar_capacidad() {
        let mut buf = RingBuffer::new(2);
        buf.push("a".into());
        buf.push("b".into());
        buf.push("c".into());
        assert_eq!(buf.snapshot(), vec!["b".to_string(), "c".to_string()]);
    }
}
