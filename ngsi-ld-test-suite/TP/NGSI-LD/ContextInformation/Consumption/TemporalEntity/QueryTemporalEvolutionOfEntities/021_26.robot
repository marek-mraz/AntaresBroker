*** Settings ***
Documentation       Check the Temporal Query Language bound semantics (CIM 009 clause 4.11):
...                 "before ... is used as an exclusive bound", "after ... is used as an
...                 inclusive bound", "between ... the lower bound of the range is inclusive
...                 and the value specified for the upper bound of the range is exclusive."
...                 Equal instants written with a 4.6.3 seconds fraction ("...00.000Z") must
...                 hit the same bounds as the fraction-less spelling. Grammar violations
...                 (unknown timerel, between without endTimeAt, a Date where a DateTime is
...                 mandated) shall be rejected.
...
...                 Antares extension TP — the official temporal TPs exercise timerel only
...                 away from the bounds.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Temporal Entities
Suite Teardown      Delete Initial Temporal Entities
Test Template       Query Temporal Entity Expecting Speed Instances


*** Variables ***
${vehicle_payload_file}=    2020-08-vehicle-temporal-representation.jsonld


*** Test Cases ***    TIMEREL    TIMEAT    ENDTIMEAT    EXPECTED_STATUS    EXPECTED_OBSERVEDATS
021_26_01 After Includes The Instance At Exactly TimeAt
    [Documentation]    4.11: after — "The specified value is used as an inclusive bound"
    [Tags]    te-query    4_11    since_v1.9.1
    after    2020-08-01T12:05:00Z    ${EMPTY}    200    2020-08-01T12:05:00Z,2020-08-01T12:07:00Z

021_26_02 After Bound Holds For A Fraction Spelling Of The Same Instant
    [Documentation]    4.11 + 4.6.3: "...12:05:00.000Z" is the same instant as
    ...    "...12:05:00Z" — the inclusive bound must not depend on the spelling
    [Tags]    te-query    4_11    since_v1.9.1
    after    2020-08-01T12:05:00.000Z    ${EMPTY}    200    2020-08-01T12:05:00Z,2020-08-01T12:07:00Z

021_26_03 Before Excludes The Instance At Exactly TimeAt
    [Documentation]    4.11: before — "The specified value is used as an exclusive bound"
    [Tags]    te-query    4_11    since_v1.9.1
    before    2020-08-01T12:05:00Z    ${EMPTY}    200    2020-08-01T12:03:00Z

021_26_04 Before Bound Holds For A Fraction Spelling Of The Same Instant
    [Documentation]    4.11 + 4.6.3: the exclusive bound must not depend on the spelling
    [Tags]    te-query    4_11    since_v1.9.1
    before    2020-08-01T12:05:00.000Z    ${EMPTY}    200    2020-08-01T12:03:00Z

021_26_05 Between Is Lower-Inclusive And Upper-Exclusive
    [Documentation]    4.11: between — "the lower bound of the range is inclusive and the
    ...    value specified for the upper bound of the range is exclusive"
    [Tags]    te-query    4_11    since_v1.9.1
    between    2020-08-01T12:03:00Z    2020-08-01T12:05:00Z    200    2020-08-01T12:03:00Z

021_26_06 Between Bounds Hold For Fraction Spellings
    [Documentation]    4.11 + 4.6.3: both bounds in "...000Z" spelling select the same
    ...    instances
    [Tags]    te-query    4_11    since_v1.9.1
    between    2020-08-01T12:03:00.000Z    2020-08-01T12:05:00.000Z    200    2020-08-01T12:03:00Z

021_26_07 Unknown Timerel Is Rejected
    [Documentation]    4.11 grammar: timerel = before / after / between only
    [Tags]    te-query    4_11    since_v1.9.1
    during    2020-08-01T12:05:00Z    ${EMPTY}    400    ${None}

021_26_08 Between Without EndTimeAt Is Rejected
    [Documentation]    4.11: endTimeAt "shall represent the end point for comparison" of
    ...    the between relation
    [Tags]    te-query    4_11    since_v1.9.1
    between    2020-08-01T12:05:00Z    ${EMPTY}    400    ${None}

021_26_09 A Date Is Not A DateTime
    [Documentation]    4.11: timeAt "shall be represented as DateTime (mandated by
    ...    clause 4.6.3)" — a reduced Date form violates that
    [Tags]    te-query    4_11    since_v1.9.1
    after    2020-08-01    ${EMPTY}    400    ${None}


*** Keywords ***
Query Temporal Entity Expecting Speed Instances
    [Arguments]    ${timerel}    ${timeAt}    ${endTimeAt}    ${expected_status_code}    ${expected_observed_ats}
    ${response}=    Query Temporal Representation Of Entities
    ...    entity_ids=${temporal_entity_id}
    ...    timerel=${timerel}
    ...    timeAt=${timeAt}
    ...    endTimeAt=${endTimeAt}
    ...    attrs=speed
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    IF    $expected_observed_ats is not None
        ${actual}=    Evaluate
        ...    sorted(i.get('observedAt') for i in (lambda v: v if isinstance(v, list) else [v])((($response.json() or [{}])[0]).get('speed') or []))
        ${expected}=    Evaluate    sorted(s for s in """${expected_observed_ats}""".split(',') if s)
        Should Be Equal    ${actual}    ${expected}
    END

Setup Initial Temporal Entities
    ${temporal_entity_id}=    Catenate    ${VEHICLE_ID_PREFIX}021-26-A
    ${create_response}=    Create Temporal Representation Of Entity
    ...    ${vehicle_payload_file}
    ...    ${temporal_entity_id}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${temporal_entity_id}

Delete Initial Temporal Entities
    Delete Temporal Representation Of Entity    ${temporal_entity_id}
