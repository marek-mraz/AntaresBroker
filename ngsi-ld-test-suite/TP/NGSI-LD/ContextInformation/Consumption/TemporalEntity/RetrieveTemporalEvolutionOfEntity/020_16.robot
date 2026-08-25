*** Settings ***
Documentation       Check that one can retrieve the temporal evolution of an entity with a VocabProperty property

Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Temporal Entity
Suite Teardown      Delete Initial Temporal Entity
Test Template       Retrieve Temporal Entity


*** Variables ***
${vehicle_payload_file}=    vehicle-vocab-property-temporal-representation.jsonld


*** Test Cases ***    REPRESENTATION    EXPECTATION_FILENAME
020_16_01 Retrieve The Normalized Temporal Representation Of An Entity With A VocabProperty Property
    [Tags]    te-retrieve    5_7_3    4_5_7    4_5_24    since_v1.7.1
    ${EMPTY}    vehicle-vocab-property-temporal-representation.json
020_16_02 Retrieve The Simplified Temporal Representation Of An Entity With A VocabProperty Property
    [Tags]    te-retrieve    5_7_3    4_5_9    4_5_24    since_v1.7.1
    temporalValues    vehicle-vocab-property-simplified-temporal-representation.json


*** Keywords ***
Retrieve Temporal Entity
    [Documentation]    Check that one can retrieve the temporal evolution of an entity with a VocabProperty property
    [Arguments]    ${representation}    ${expectation_filename}
    ${response}=    Retrieve Temporal Representation Of Entity
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    options=${representation}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing EntityTemporal element
    ...    ${expectation_filename}
    ...    ${temporal_entity_representation_id}
    ...    ${response.json()}

Create Temporal Entity
    ${temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    Create Temporal Representation Of Entity    ${vehicle_payload_file}    ${temporal_entity_representation_id}
    Set Suite Variable    ${temporal_entity_representation_id}

Delete Initial Temporal Entity
    Delete Temporal Representation Of Entity    ${temporal_entity_representation_id}
