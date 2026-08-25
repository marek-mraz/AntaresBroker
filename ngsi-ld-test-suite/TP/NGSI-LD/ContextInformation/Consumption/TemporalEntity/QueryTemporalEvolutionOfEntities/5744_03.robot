*** Settings ***
Documentation       Verify 5.7.4.4 S4 (and S7): "If the Scope query is
...                 present, from S3, select those Entities whose Entity
...                 Scope instances match the Scope query (as mandated by
...                 clause 4.19, for an example see annex C, clause
...                 C.5.16)" — with the 4.18 validity semantics: "a given
...                 Scope is considered valid from the time it has been set
...                 until the time it has been explicitly removed by an
...                 update or delete operation". Covers exact match,
...                 non-match, subtree '#', any-scope '/#', '|'
...                 alternatives, ';' conjunction, and validity over time
...                 (scope set mid-window, replaced mid-window, set only
...                 after the window). Antares extension TP.

Library             RequestsLibrary
Library             Collections
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Setup         Create Fixture Entities
Suite Teardown      Delete Fixture Entities


*** Variables ***
${ab}=          urn:ngsi-ld:Vehicle:qs5744-ab
${xx}=          urn:ngsi-ld:Vehicle:qs5744-xx
${bare}=        urn:ngsi-ld:Vehicle:qs5744-bare
${both}=        urn:ngsi-ld:Vehicle:qs5744-both
${mid}=         urn:ngsi-ld:Vehicle:qs5744-mid
${rep}=         urn:ngsi-ld:Vehicle:qs5744-rep
${late}=        urn:ngsi-ld:Vehicle:qs5744-late
${window}=      timerel=between&timeAt=2026-03-01T12:00:00Z&endTimeAt=2026-03-01T13:00:00Z


*** Test Cases ***
5744_03_01 Exact Scope Match Selects Entities
    [Documentation]    5.7.4.4 S4 + 4.19: scopeQ=/A/B selects the entities
    ...    whose scope is (or contains) /A/B; others are excluded.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Scoped    /A/B
    Should Contain    ${response.text}    ${ab}
    Should Contain    ${response.text}    ${both}
    Should Not Contain    ${response.text}    ${xx}
    Should Not Contain    ${response.text}    ${bare}
    Should Not Contain    ${response.text}    ${late}

5744_03_02 Non-Matching Scope Excludes Everything
    [Documentation]    5.7.4.4 S4: a scopeQ no entity's scope matches
    ...    returns none of the fixtures.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Scoped    /Y
    Should Not Contain    ${response.text}    qs5744

5744_03_03 Subtree Match With Hash
    [Documentation]    4.19: a trailing '#' matches the node and its
    ...    subtree — /A/# selects /A/B; /X stays excluded.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Scoped    /A/%23
    Should Contain    ${response.text}    ${ab}
    Should Contain    ${response.text}    ${both}
    Should Not Contain    ${response.text}    ${xx}

5744_03_04 Any Scope Excludes Unscoped And Window-Invalid
    [Documentation]    4.19 '/#' matches any non-empty scope — but only if
    ...    a scope was VALID within the window (4.18): the unscoped entity
    ...    and the one whose scope was set only after the window end must
    ...    not appear.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Scoped    /%23
    Should Contain    ${response.text}    ${ab}
    Should Contain    ${response.text}    ${xx}
    Should Contain    ${response.text}    ${both}
    Should Contain    ${response.text}    ${mid}
    Should Contain    ${response.text}    ${rep}
    Should Not Contain    ${response.text}    ${bare}
    Should Not Contain    ${response.text}    ${late}

5744_03_05 Scope Alternatives
    [Documentation]    4.19: '|' separates alternatives — /X|/M selects
    ...    both scoped entities, the /A/B one stays excluded.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Scoped    /X%7C/M
    Should Contain    ${response.text}    ${xx}
    Should Contain    ${response.text}    ${mid}
    Should Not Contain    ${response.text}    ${ab}

5744_03_06 Scope Conjunction
    [Documentation]    4.19: '(a;b)' requires both scopes — only the
    ...    entity carrying /A/B AND /C matches.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Scoped    (/A/B%3B/C)
    Should Contain    ${response.text}    ${both}
    Should Not Contain    ${response.text}    ${ab}

5744_03_07 Scope Set Mid Window Bounds Instances
    [Documentation]    C.5.16 (A8311): a scope set within the window
    ...    qualifies the entity, but only Attribute instances observed
    ...    after the scope became valid are included.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Scoped    /M
    Should Contain    ${response.text}    ${mid}
    Should Contain    ${response.text}    midpost
    Should Not Contain    ${response.text}    midpre

5744_03_08 Replaced Scope Old Interval
    [Documentation]    4.18: a scope replaced by an update stops being
    ...    valid — the old scope's query sees only instances before the
    ...    replacement.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Scoped    /R
    Should Contain    ${response.text}    ${rep}
    Should Contain    ${response.text}    repold
    Should Not Contain    ${response.text}    repnew

5744_03_09 Replaced Scope New Interval
    [Documentation]    4.18: the replacing scope is valid from its
    ...    set-time — its query sees only the later instance.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Scoped    /S
    Should Contain    ${response.text}    ${rep}
    Should Contain    ${response.text}    repnew
    Should Not Contain    ${response.text}    repold

5744_03_10 Scope Valid Only After Window Never Matches
    [Documentation]    4.18/5.7.4.4 S4: a scope set only after the window
    ...    end was never valid within it — the entity must not be
    ...    returned even though it has in-window Attribute instances.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Scoped    /Z
    Should Not Contain    ${response.text}    qs5744

5744_03_11 Valid Scope Carried Into The Representation
    [Documentation]    C.5.16 (B9211): the scope valid at the window start
    ...    was set before it — its value is still presented with the
    ...    entity.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    Query Scoped    /A/B
    Should Contain    ${response.text}    /A/B


*** Keywords ***
Query Scoped
    [Arguments]    ${sq}
    ${response}=    GET
    ...    url=${temporal_api_url}/temporal/entities
    ...    params=type=Vehicle&scopeQ=${sq}&${window}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    RETURN    ${response}

Create Temporal Entity
    [Arguments]    ${payload}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST
    ...    url=${temporal_api_url}/temporal/entities
    ...    data=${payload}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    201    ${response.status_code}

Create Fixture Entities
    Create Temporal Entity    {"id": "${ab}", "type": "Vehicle", "scope": [{"type": "Property", "value": "/A/B", "observedAt": "2026-03-01T10:00:00Z"}], "speed": [{"type": "Property", "value": "abval", "observedAt": "2026-03-01T12:05:00Z"}]}
    Create Temporal Entity    {"id": "${xx}", "type": "Vehicle", "scope": [{"type": "Property", "value": "/X", "observedAt": "2026-03-01T10:00:00Z"}], "speed": [{"type": "Property", "value": "xxval", "observedAt": "2026-03-01T12:05:00Z"}]}
    Create Temporal Entity    {"id": "${bare}", "type": "Vehicle", "speed": [{"type": "Property", "value": "bareval", "observedAt": "2026-03-01T12:05:00Z"}]}
    Create Temporal Entity    {"id": "${both}", "type": "Vehicle", "scope": [{"type": "Property", "value": ["/A/B", "/C"], "observedAt": "2026-03-01T10:00:00Z"}], "speed": [{"type": "Property", "value": "bothval", "observedAt": "2026-03-01T12:05:00Z"}]}
    Create Temporal Entity    {"id": "${mid}", "type": "Vehicle", "scope": [{"type": "Property", "value": "/M", "observedAt": "2026-03-01T12:10:00Z"}], "speed": [{"type": "Property", "value": "midpre", "observedAt": "2026-03-01T12:05:00Z"}, {"type": "Property", "value": "midpost", "observedAt": "2026-03-01T12:15:00Z"}]}
    Create Temporal Entity    {"id": "${rep}", "type": "Vehicle", "scope": [{"type": "Property", "value": "/R", "observedAt": "2026-03-01T10:00:00Z"}, {"type": "Property", "value": "/S", "observedAt": "2026-03-01T12:30:00Z"}], "speed": [{"type": "Property", "value": "repold", "observedAt": "2026-03-01T12:15:00Z"}, {"type": "Property", "value": "repnew", "observedAt": "2026-03-01T12:45:00Z"}]}
    Create Temporal Entity    {"id": "${late}", "type": "Vehicle", "scope": [{"type": "Property", "value": "/Z", "observedAt": "2026-03-01T14:00:00Z"}], "speed": [{"type": "Property", "value": "lateval", "observedAt": "2026-03-01T12:05:00Z"}]}

Delete Fixture Entities
    FOR    ${id}    IN    ${ab}    ${xx}    ${bare}    ${both}    ${mid}    ${rep}    ${late}
        ${response}=    DELETE    url=${temporal_api_url}/temporal/entities/${id}    expected_status=any
    END
