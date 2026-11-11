WHENEVER SQLERROR EXIT FAILURE
SET ECHO OFF VERIFY OFF SERVEROUTPUT ON PAGESIZE 0 FEEDBACK OFF
DECLARE
  active_count PLS_INTEGER;
  seen_active BOOLEAN := FALSE;
  stopped BOOLEAN := FALSE;
BEGIN
  FOR sample IN 1..200 LOOP
    SELECT COUNT(*) INTO active_count
    FROM v$session s
    WHERE s.username = 'RAWDEADLINE'
      AND s.module = 'sutura.raw-deadline-probe'
      AND s.action = 'long-query'
      AND s.status = 'ACTIVE'
      AND EXISTS (
        SELECT 1 FROM v$sql q
        WHERE q.sql_id = s.sql_id
          AND q.sql_text LIKE 'SELECT /* sutura_raw_deadline_probe */ SUM(ORA_HASH(LEVEL))%'
      );
    IF active_count > 0 AND NOT seen_active THEN
      seen_active := TRUE;
      DBMS_OUTPUT.PUT_LINE('server_active_sample=' || sample);
    ELSIF active_count = 0 AND seen_active THEN
      stopped := TRUE;
      DBMS_OUTPUT.PUT_LINE('server_stopped_sample=' || sample);
      EXIT;
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
