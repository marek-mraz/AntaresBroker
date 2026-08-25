*** Settings ***
Documentation       Check that one can retrieve the temporal evolution of an entity matching the given NGSI-LD temporal query

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Temporal Entity
Test Teardown       Delete Initial Temporal Entity
Test Template       Retrieve the temporal evolution of an entity matching the given NGSI-LD temporal query


*** Variables ***
${vehicle_payload_file}=    2020-08-vehicle-temporal-representation.jsonld


*** Test Cases ***    TIMEREL    TIMEAT    ENDTIMEAT    VEHICLE_EXPECTATION_FILE
020_04_01 After
    [Tags]    te-retrieve    5_7_3
    after    2020-08-01T13:03:00Z    ${EMPTY}    vehicle-temporal-representation-020-04-01.jsonld
020_04_02 Before
    [Tags]    te-retrieve    5_7_3
    before    2020-08-01T12:05:00Z    ${EMPTY}    vehicle-temporal-representation-020-04-02.jsonld
020_04_03 Between
    [Tags]    te-retrieve    5_7_3
    between    2020-08-01T12:00:00Z    2020-08-01T13:00:00Z    vehicle-temporal-representation-020-04-03.jsonld


*** Keywords ***
Retrieve the temporal evolution of an entity matching the given NGSI-LD temporal query
    [Documentation]    Check that one can retrieve the temporal evolution of an entity matching the given NGSI-LD temporal query
    [Arguments]    ${timerel}    ${timeat}    ${endtimeat}    ${vehicle_expectation_file}
    ${response}=    Retrieve Temporal Representation Of Entity
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    timerel=${timerel}
    ...    timeAt=${timeat}
    ...    endTimeAt=${endtimeat}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing EntityTemporal element
    ...    ${vehicle_expectation_file}
    ...    ${temporal_entity_representation_id}
    ...    ${response.json()}

Create Temporal Entity
    ${temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    ${create_response}=    Create Temporal Representation Of Entity
    ...    ${vehicle_payload_file}
    ...    ${temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response.status_code}
    Set Test Variable    ${temporal_entity_representation_id}

Delete Initial Temporal Entity
    Delete Temporal Representation Of Entity    ${temporal_entity_representation_id}
