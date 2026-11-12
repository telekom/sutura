WHENEVER SQLERROR EXIT FAILURE
SET ECHO OFF VERIFY OFF SERVEROUTPUT ON PAGESIZE 0 FEEDBACK OFF
DECLARE
  active_count PLS_INTEGER;
  session_count PLS_INTEGER;
  probe_sid NUMBER;
  probe_serial NUMBER;
  probe_sql_id VARCHAR2(13);
  seen_active BOOLEAN := FALSE;
  discarded BOOLEAN := FALSE;
  active_at NUMBER;
  started_at NUMBER := DBMS_UTILITY.GET_TIME;
BEGIN
  FOR sample IN 1..400 LOOP
    IF NOT seen_active THEN
      SELECT COUNT(*), MIN(s.sid), MIN(s.serial#), MIN(s.sql_id)
      INTO active_count, probe_sid, probe_serial, probe_sql_id
      FROM v$session s
      WHERE s.username = '&fixture_user' AND s.status = 'ACTIVE'
        AND EXISTS (
          SELECT 1 FROM v$sql q WHERE q.sql_id = s.sql_id
            AND q.sql_text = 'SELECT /*+ NO_PARALLEL */ /* sutura_raw_deadline_probe */ SUM(ORA_HASH(a.n + 1000 * b.n + 1000000 * c.n)) FROM (SELECT LEVEL n FROM dual CONNECT BY LEVEL <= 1000) a CROSS JOIN (SELECT LEVEL n FROM dual CONNECT BY LEVEL <= 1000) b CROSS JOIN (SELECT LEVEL n FROM dual CONNECT BY LEVEL <= 1000) c'
        );
      IF active_count > 1 THEN
        RAISE_APPLICATION_ERROR(-20004, 'more than one fixture query matched');
      ELSIF active_count = 1 THEN
        IF (DBMS_UTILITY.GET_TIME - started_at) * 10 > 3000 THEN
          RAISE_APPLICATION_ERROR(-20005, 'initial query binding exceeded three seconds');
        END IF;
        DBMS_OUTPUT.PUT_LINE('server_first_active_ms=' || (DBMS_UTILITY.GET_TIME - started_at) * 10);
        seen_active := TRUE;
        active_at := DBMS_UTILITY.GET_TIME;
        DBMS_OUTPUT.PUT_LINE('server_probe_sid=' || probe_sid);
        DBMS_OUTPUT.PUT_LINE('server_probe_serial=' || probe_serial);
        DBMS_OUTPUT.PUT_LINE('server_probe_sql_id=' || probe_sql_id);
        DBMS_OUTPUT.PUT_LINE('server_active=true');
      END IF;
    ELSE
      SELECT COUNT(*), COUNT(CASE WHEN s.status = 'ACTIVE' THEN 1 END)
      INTO session_count, active_count FROM v$session s
      WHERE s.sid = probe_sid AND s.serial# = probe_serial;
      IF (DBMS_UTILITY.GET_TIME - active_at) * 10 > 6000 THEN
        DBMS_OUTPUT.PUT_LINE('server_final_active_count=' || active_count);
        DBMS_OUTPUT.PUT_LINE('server_final_session_count=' || session_count);
        RAISE_APPLICATION_ERROR(-20006, 'the owned session was not discarded within six seconds');
      END IF;
      IF session_count = 0 THEN
        discarded := TRUE;
        DBMS_OUTPUT.PUT_LINE('server_active_to_discarded_ms=' || (DBMS_UTILITY.GET_TIME - active_at) * 10);
        EXIT;
      END IF;
    END IF;
    DBMS_LOCK.SLEEP(0.1);
  END LOOP;
  IF NOT seen_active OR NOT discarded THEN
    RAISE_APPLICATION_ERROR(-20001, 'no active-to-discarded session witness');
  END IF;
  DBMS_OUTPUT.PUT_LINE('server_session_discarded=true');
END;
/
EXIT
