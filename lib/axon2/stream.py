from axon2.v2026 import Node

class EnvelopeStream:
    """
    An EDIFACT-inspired stream validator for AXON.
    
    Wraps an iterator (e.g. from iloads2026) and strictly enforces that the data
    is securely wrapped within a StreamStart and StreamEnd node. Validates record
    counts to prevent truncation attacks in B2B ledgers.
    """

    def __init__(self, iterator):
        self.iterator = iter(iterator)
        self.in_stream = False
        self.count = 0

    def __iter__(self):
        for item in self.iterator:
            if isinstance(item, Node):
                if item.name == 'StreamStart':
                    if self.in_stream:
                        raise ValueError('Received StreamStart but a stream is already open')
                    self.in_stream = True
                    self.count = 0
                    continue
                elif item.name == 'StreamEnd':
                    if not self.in_stream:
                        raise ValueError('Received StreamEnd without a prior StreamStart')
                    expected = item.attributes.get('count', None)
                    if expected is not None and expected != self.count:
                        raise ValueError(f'StreamEnd count mismatch: expected {expected}, got {self.count}')
                    self.in_stream = False
                    continue
            if not self.in_stream:
                raise ValueError(f'Received data outside of a stream envelope: {item}')
            self.count += 1
            yield item
        if self.in_stream:
            raise ValueError('Stream ended abruptly without a StreamEnd envelope node')