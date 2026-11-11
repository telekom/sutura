WHENEVER SQLERROR EXIT FAILURE
SET ECHO OFF VERIFY OFF SERVEROUTPUT ON PAGESIZE 0 FEEDBACK OFF
DECLARE
  active_count PLS_INTEGER;
  probe_sid NUMBER;
  probe_serial NUMBER;
  probe_sql_id VARCHAR2(13);
  seen_active BOOLEAN := FALSE;
  stopped BOOLEAN := FALSE;
  started_at NUMBER := DBMS_UTILITY.GET_TIME;
  active_at NUMBER;
BEGIN
  FOR sample IN 1..200 LOOP
    IF NOT seen_active THEN
      SELECT COUNT(*), MIN(s.sid), MIN(s.serial#), MIN(s.sql_id)
      INTO active_count, probe_sid, probe_serial, probe_sql_id
      FROM v$session s
      WHERE s.username = 'RAWDEADLINE'
        AND s.module = 'sutura.raw-deadline-probe'
        AND s.action = 'long-query'
        AND s.status = 'ACTIVE'
        AND EXISTS (
          SELECT 1 FROM v$sql q
          WHERE q.sql_id = s.sql_id
            AND q.sql_text = 'SELECT /*+ NO_PARALLEL */ /* sutura_raw_deadline_probe */ SUM(ORA_HASH(a.n + 1000 * b.n + 1000000 * c.n)) FROM (SELECT LEVEL n FROM dual CONNECT BY LEVEL <= 1000) a CROSS JOIN (SELECT LEVEL n FROM dual CONNECT BY LEVEL <= 1000) b CROSS JOIN (SELECT LEVEL n FROM dual CONNECT BY LEVEL <= 1000) c'
        );
      IF active_count > 1 THEN
        RAISE_APPLICATION_ERROR(-20004, 'more than one fixture query matched');
      ELSIF active_count = 1 THEN
        IF (DBMS_UTILITY.GET_TIME - started_at) * 10 >= 3000 THEN
          RAISE_APPLICATION_ERROR(-20005, 'fixture execution was not observed before the timeout window');
        END IF;
        seen_active := TRUE;
        active_at := DBMS_UTILITY.GET_TIME;
        DBMS_OUTPUT.PUT_LINE('server_active_sample=' || sample);
        DBMS_OUTPUT.PUT_LINE('server_active_elapsed_ms=' || (DBMS_UTILITY.GET_TIME - started_at) * 10);
        DBMS_OUTPUT.PUT_LINE('server_probe_sid=' || probe_sid);
        DBMS_OUTPUT.PUT_LINE('server_probe_serial=' || probe_serial);
        DBMS_OUTPUT.PUT_LINE('server_probe_sql_id=' || probe_sql_id);
      END IF;
    ELSE
      SELECT COUNT(*) INTO active_count FROM v$session s
      WHERE s.sid = probe_sid AND s.serial# = probe_serial AND s.status = 'ACTIVE';
      IF (DBMS_UTILITY.GET_TIME - active_at) * 10 > 6000 THEN
        DBMS_OUTPUT.PUT_LINE('server_final_active_count=' || active_count);
        DBMS_OUTPUT.PUT_LINE('server_final_elapsed_since_active_ms=' || (DBMS_UTILITY.GET_TIME - active_at) * 10);
        RAISE_APPLICATION_ERROR(-20006, 'the owned session did not stop within six seconds of observation');
      END IF;
      IF active_count = 0 THEN
        stopped := TRUE;
        DBMS_OUTPUT.PUT_LINE('server_stopped_sample=' || sample);
        DBMS_OUTPUT.PUT_LINE('server_stopped_elapsed_ms=' || (DBMS_UTILITY.GET_TIME - started_at) * 10);
        DBMS_OUTPUT.PUT_LINE('server_active_to_stopped_ms=' || (DBMS_UTILITY.GET_TIME - active_at) * 10);
        EXIT;
      END IF;
    END IF;
    DBMS_LOCK.SLEEP(0.1);
  END LOOP;
  IF NOT seen_active THEN
    RAISE_APPLICATION_ERROR(-20001, 'the probe never observed server execution');
  END IF;
  IF NOT stopped THEN
    RAISE_APPLICATION_ERROR(-20002, 'server execution remained active');
  END IF;
  DBMS_OUTPUT.PUT_LINE('server_active_to_stopped=true');
END;
/
EXIT
