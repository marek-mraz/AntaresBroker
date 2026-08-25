*** Settings ***
Documentation       Check that an error is raised if one adds an attribute to a non-existent entity

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Variables ***
${filename}=                vehicle-temporal-representation.jsonld
${fragment_filename}=       vehicle-temporal-representation-fragment.jsonld
${status_code}=             404


*** Test Cases ***
014_03_01 Add Attribute To Temporal Entity
    [Documentation]    Check that an error is raised if one adds an attribute to a non-existent entity
    [Tags]    tea-append    5_6_12
    ${not_found_temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    ${response}=    Append Attribute To Temporal Entity
    ...    ${not_found_temporal_entity_representation_id}
    ...    ${fragment_filename}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    ${status_code}    ${response.status_code}
