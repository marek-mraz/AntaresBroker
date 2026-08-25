*** Settings ***
Documentation       Check the TemporalQuery data type in its JSON form (CIM 009 clause 5.2.21,
...                 Table 5.2.21-1) as carried inside a Query body (clause 5.2.23) on
...                 POST /temporal/entityOperations/query: lastN is a positive integer,
...                 timerel is limited to before/after/between, endTimeAt is mandatory for
...                 between, timeproperty is limited to the clause 4.8 names, aggrMethods
...                 is a comma separated list of string (string and string-array spellings)
...                 and aggrPeriodDuration must be a valid duration.
...
...                 Antares extension TP — the official POST-query TPs only exercise a
...                 valid timerel/timeAt pair.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Temporal Entities
Suite Teardown      Delete Initial Temporal Entities
Test Template       Query Via Post Expecting


*** Variables ***
${vehicle_payload_file}=    2020-08-vehicle-temporal-representation.jsonld


*** Test Cases ***    QUERY_FILE    EXPECTED_STATUS    MUST_CONTAIN    MUST_NOT_CONTAIN
021_27_01 LastN In The JSON TemporalQuery Is Honoured
    [Documentation]    5.2.21: lastN — "Only the last n instances, per Attribute, per
    ...    Entity (under the specified time interval) shall be retrieved"
    [Tags]    te-query    5_2_21    since_v1.9.1
    temporalQ-json-lastN-one-query.jsonld    200    2020-08-01T12:07:00Z    2020-08-01T12:03:00Z

021_27_02 LastN Zero Is Outside The Positive Integer Value Space
    [Documentation]    5.2.21 Table 5.2.21-1: lastN data type is "Positive integer"
    [Tags]    te-query    5_2_21    since_v1.9.1
    temporalQ-json-lastN-zero-query.jsonld    400    BadRequestData    ${None}

021_27_03 A Fractional LastN Is Not An Integer
    [Documentation]    5.2.21 Table 5.2.21-1: lastN data type is "Positive integer"
    [Tags]    te-query    5_2_21    since_v1.9.1
    temporalQ-json-lastN-fractional-query.jsonld    400    BadRequestData    ${None}

021_27_04 Timerel Is Limited To Before After Between
    [Documentation]    5.2.21 Table 5.2.21-1: timerel allowed values are "before",
    ...    "after" and "between"
    [Tags]    te-query    5_2_21    since_v1.9.1
    temporalQ-json-invalid-timerel-query.jsonld    400    BadRequestData    ${None}

021_27_05 Between Requires EndTimeAt
    [Documentation]    5.2.21 Table 5.2.21-1: endTimeAt "Cardinality shall be 1 if
    ...    timerel is equal to between"
    [Tags]    te-query    5_2_21    since_v1.9.1
    temporalQ-json-between-no-end-query.jsonld    400    BadRequestData    ${None}

021_27_06 Timeproperty Is Limited To The Four Temporal Property Names
    [Documentation]    5.2.21 Table 5.2.21-1: timeproperty allowed values are
    ...    "observedAt", "createdAt", "modifiedAt" and "deletedAt"
    [Tags]    te-query    5_2_21    since_v1.9.1
    temporalQ-json-invalid-timeproperty-query.jsonld    400    BadRequestData    ${None}

021_27_07 AggrMethods As A String Array Is Honoured
    [Documentation]    5.2.21 Table 5.2.21-1: aggrMethods — "Each String represents an
    ...    aggregation method, as defined by clause 4.5.19"
    [Tags]    te-query    5_2_21    since_v1.9.1
    temporalQ-json-aggrMethods-array-query.jsonld    200    avg    instanceId

021_27_08 AggrMethods As A Comma Separated String Is Honoured
    [Documentation]    5.2.21 Table 5.2.21-1: aggrMethods data type is "Comma separated
    ...    list of string"
    [Tags]    te-query    5_2_21    since_v1.9.1
    temporalQ-json-aggrMethods-string-query.jsonld    200    avg    instanceId

021_27_09 An Invalid AggrPeriodDuration Is Rejected
    [Documentation]    5.2.21 Table 5.2.21-1: aggrPeriodDuration "represents the duration
    ...    of each period used for the aggregation as defined by clause 4.5.19"
    [Tags]    te-query    5_2_21    since_v1.9.1
    temporalQ-json-bad-aggrPeriodDuration-query.jsonld    400    aggrPeriodDuration    ${None}


*** Keywords ***
Query Via Post Expecting
    [Arguments]    ${query_file}    ${expected_status_code}    ${must_contain}    ${must_not_contain}
    ${response}=    Query Temporal Representation Of Entities Via Post
    ...    ${query_file}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    ${body}=    Evaluate    $response.text
    IF    $must_contain is not None
        Should Contain    ${body}    ${must_contain}
    END
    IF    $must_not_contain is not None
        Should Not Contain    ${body}    ${must_not_contain}
    END

Setup Initial Temporal Entities
    ${temporal_entity_id}=    Catenate    ${VEHICLE_ID_PREFIX}021-27-A
    ${create_response}=    Create Temporal Representation Of Entity
    ...    ${vehicle_payload_file}
    ...    ${temporal_entity_id}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${temporal_entity_id}

Delete Initial Temporal Entities
    Delete Temporal Representation Of Entity    ${temporal_entity_id}
