*** Settings ***
Documentation       Check that one can retrieve the temporal evolution of an entity and pick or omit some attributes

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Temporal Entity
Test Teardown       Delete Initial Temporal Entity
Test Template       Retrieve the temporal evolution of an entity with pick and omit


*** Variables ***
${vehicle_payload_file}=        2020-08-vehicle-temporal-representation.jsonld
${vehicle_expectation_file}=    vehicle-temporal-representation-020-03.jsonld


*** Test Cases ***    PICK    OMIT    EXPECTATION_FILENAME
020_21_01 RetrieveWithPickOnAttributes
    [Documentation]    Check that one can retrieve an entity with pick on attributes
    [Tags]    te-retrieve    5_7_3    4_21    since_v1.8.1
    speed    ${EMPTY}    pick-omit/vehicle-pick-speed.json
020_21_02 RetrieveWithOmitOnAttributes
    [Documentation]    Check that one can retrieve an entity with omit on attributes
    [Tags]    te-retrieve    5_7_3    4_21    since_v1.8.1
    ${EMPTY}    speed    pick-omit/vehicle-omit-speed.json
020_21_03 RetrieveWithPickOnAttributesAndCoreMembers
    [Documentation]    Check that one can retrieve an entity with pick on attributes and core members
    [Tags]    te-retrieve    5_7_3    4_21    since_v1.8.1
    id,speed    ${EMPTY}    pick-omit/vehicle-pick-id-speed.json
020_21_04 RetrieveWithOmitOnAttributesAndCoreMembers
    [Documentation]    Check that one can retrieve an entity with omit on attributes and core members
    [Tags]    te-retrieve    5_7_3    4_21    since_v1.8.1
    ${EMPTY}    type,speed    pick-omit/vehicle-omit-type-speed.json


*** Keywords ***
Retrieve the temporal evolution of an entity with pick and omit
    [Documentation]    Check that one can retrieve the temporal evolution of an entity and pick or omit some attributes
    [Arguments]    ${pick}    ${omit}    ${expectation_filename}

    ${response}=    Retrieve Temporal Representation Of Entity
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    pick=${pick}
    ...    omit=${omit}
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing EntityTemporal element
    ...    ${expectation_filename}
    ...    ${temporal_entity_representation_id}
    ...    ${response.json()}

Create Temporal Entity
    ${temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    ${create_response}=    Create Temporal Representation Of Entity
    ...    ${vehicle_payload_file}
    ...    ${temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${temporal_entity_representation_id}

Delete Initial Temporal Entity
    Delete Temporal Representation Of Entity    ${temporal_entity_representation_id}
