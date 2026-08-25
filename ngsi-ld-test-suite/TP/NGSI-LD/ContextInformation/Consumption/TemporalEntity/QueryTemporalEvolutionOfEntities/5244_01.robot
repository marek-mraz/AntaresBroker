*** Settings ***
Documentation       Check the AggregationParams data type (CIM 009 clause 5.2.44,
...                 Table 5.2.44-1) inside a Query body on
...                 POST /temporal/entityOperations/query: aggrMethods entries are
...                 limited to the 4.5.19 methods, aggrPeriodDuration must be a valid
...                 duration, and a conformant aggrParams yields an aggregatedValues
...                 response.
...
...                 Antares extension TP — no official TP sends the aggrParams member.

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
5244_01_01 A Conformant AggrParams Yields Aggregated Values
    [Documentation]    5.2.44: aggrMethods "Each String represents an aggregation
    ...    method, as defined by clause 4.5.19"
    [Tags]    te-query    5_2_44    since_v1.9.1
    aggrParams-avg-query.jsonld    200    avg    instanceId

5244_01_02 An Unknown Aggregation Method Is Rejected
    [Tags]    te-query    5_2_44    since_v1.9.1
    aggrParams-bogus-method-query.jsonld    400    aggrMethods    ${None}

5244_01_03 An Invalid AggrPeriodDuration Is Rejected
    [Documentation]    5.2.44: aggrPeriodDuration "represents the duration of each
    ...    period used for the aggregation as defined by clause 4.5.19"
    [Tags]    te-query    5_2_44    since_v1.9.1
    aggrParams-bad-duration-query.jsonld    400    aggrPeriodDuration    ${None}


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
    ${temporal_entity_id}=    Catenate    ${VEHICLE_ID_PREFIX}5244-A
    ${create_response}=    Create Temporal Representation Of Entity
    ...    ${vehicle_payload_file}
    ...    ${temporal_entity_id}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${temporal_entity_id}

Delete Initial Temporal Entities
    Delete Temporal Representation Of Entity    ${temporal_entity_id}
