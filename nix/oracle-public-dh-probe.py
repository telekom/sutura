"""Public ANO parameters only; no login, private exponent, shared key, or record payload."""
import os
import socket
import struct


def tlv(kind, value):
    return struct.pack('!HH', len(value), kind) + value


def service(kind, fields):
    return struct.pack('!HHI', kind, len(fields), 0) + b''.join(fields)


def exact(sock, n):
    out = bytearray()
    while len(out) < n:
        data = sock.recv(n - len(out))
        if not data:
            raise RuntimeError('peer closed before public parameters')
        out.extend(data)
    return bytes(out)


def packet(sock, full):
    header = exact(sock, 8)
    size = struct.unpack('!I' if full else '!H', header[:4] if full else header[:2])[0]
    if not 8 <= size <= 65535:
        raise RuntimeError('invalid TNS packet size')
    return header[4], exact(sock, size - 8)


def main():
    host = os.environ.get('ORACLE_HOST', '127.0.0.1')
    port = int(os.environ.get('ORACLE_PORT', '1521'))
    name = os.environ.get('ORACLE_SERVICE', 'XEPDB1')
    descriptor = f'(DESCRIPTION=(ADDRESS=(PROTOCOL=TCP)(HOST={host})(PORT={port}))(CONNECT_DATA=(SERVICE_NAME={name})))'.encode()
    body = struct.pack('!HHHHHHHHHHI', 319, 300, 1, 8192, 8192, 0x4f98, 0, 1, len(descriptor), 74, 0)
    body += b'\x81\x81' + bytes(24) + struct.pack('!IIII', 8192, 8192, 0, 0)
    assert len(body) == 66
    with socket.create_connection((host, port), timeout=15) as sock:
        sock.settimeout(15)
        sock.sendall(struct.pack('!HHBBH', 74 + len(descriptor), 0, 1, 0, 0) + body + descriptor)
        kind, accepted = packet(sock, False)
        if kind == 11:  # resend
            sock.sendall(struct.pack('!HHBBH', 74 + len(descriptor), 0, 1, 0, 0) + body + descriptor)
            kind, accepted = packet(sock, False)
        if kind != 2:
            raise RuntimeError(f'TNS did not accept: packet type {kind}')
        print('accept version', int.from_bytes(accepted[:2], 'big'), 'flags', accepted[14:16].hex(), flush=True)
        version = tlv(5, bytes.fromhex('17000000'))
        array = bytes.fromhex('deadbeef0003') + struct.pack('!I4H', 4, 4, 1, 2, 3)
        services = service(4, [version, tlv(1, bytes.fromhex('0000101c66ec28ea')), tlv(1, array)])
        services += service(1, [version, tlv(3, bytes.fromhex('e0e1')), tlv(6, bytes.fromhex('fcff'))])
        services += service(2, [version, tlv(1, bytes([17, 16, 15])), tlv(2, b'\1')])
        services += service(3, [version, tlv(1, bytes([4, 6, 5]))])
        ano = struct.pack('!IHIHB', 0xdeadbeef, 13 + len(services), 0x17000000, 4, 0) + services
        sock.sendall(struct.pack('!IBBH', len(ano) + 10, 6, 0, 0) + bytes(2) + ano)
        kind, data = packet(sock, True)
        if kind != 6 or len(data) < 15:
            raise RuntimeError(f'no ANO response: packet type {kind}')
        data = data[2:]
        magic, length, version, count, flags = struct.unpack('!IHIHB', data[:13])
        if magic != 0xdeadbeef or not 13 <= length <= 4096 or not 1 <= count <= 4:
            raise RuntimeError('bad ANO header')
        while len(data) < length:
            kind, extra = packet(sock, True)
            if kind != 6 or len(extra) < 2 or len(data) + len(extra) - 2 > 4096:
                raise RuntimeError('bad ANO continuation')
            data += extra[2:]
        print('ANO version', hex(version), 'services', count, 'flags', flags, flush=True)
        refused = flags != 0
        found_group = False
        offset = 13
        for _ in range(count):
            kind, num_fields, error = struct.unpack('!HHI', data[offset:offset + 8])
            offset += 8
            print('service', kind, 'field-count', num_fields, 'status', error, flush=True)
            refused |= error != 0
            if not 0 <= num_fields <= 8:
                raise RuntimeError('unbounded ANO field count')
            fields = []
            for _ in range(num_fields):
                size, field_type = struct.unpack('!HH', data[offset:offset + 4])
                offset += 4
                if size > 512 or offset + size > len(data):
                    raise RuntimeError('unbounded ANO field')
                fields.append((field_type, data[offset:offset + size]))
                offset += size
            if fields:
                print('service-version', kind, hex(int.from_bytes(fields[0][1], 'big')), flush=True)
            if kind == 4 and len(fields) >= 3:
                print('supervisor-services', fields[2][1].hex(), flush=True)
            if kind == 2 and len(fields) >= 2:
                print('encryption-id', int.from_bytes(fields[1][1], 'big'), flush=True)
            if kind == 3 and len(fields) >= 2:
                print('integrity-id', int.from_bytes(fields[1][1], 'big'), flush=True)
                if len(fields) >= 8:
                    found_group = True
                    print('DH bits', int.from_bytes(fields[2][1], 'big'), int.from_bytes(fields[3][1], 'big'), flush=True)
                    print('DH generator', fields[4][1].hex(), flush=True)
                    print('DH modulus', fields[5][1].hex(), flush=True)
                    print('DH public-length', len(fields[6][1]), 'IV-length', len(fields[7][1]), flush=True)
        if offset != length:
            raise RuntimeError('ANO trailing bytes')
        if refused or not found_group:
            raise RuntimeError('server refused negotiation or supplied no public DH group')


if __name__ == '__main__':
    main()
