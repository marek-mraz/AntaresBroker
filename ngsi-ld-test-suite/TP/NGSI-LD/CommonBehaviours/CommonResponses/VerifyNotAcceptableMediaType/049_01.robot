*** Settings ***
Documentation       Verify throwing 406 HTTP status code (Not Acceptable Media Type) if the "Accept" header does not imply "application/json" nor "application/ld+json"

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Variables ***
${accept}=          application/xml
${status_code}=     406


*** Test Cases ***
049_01_01 Endpoint Get /entities/{entityId}
    [Documentation]    Verify throwing 406 HTTP status code (Not Acceptable Media Type) if the "Accept" header does not imply "application/json" nor "application/ld+json" (get /entities/{entityId})
    [Tags]    e-query    cb-noacceptable-medtype    6_3_4
    ${entity_id}=    Generate Random Building Entity Id
    ${response}=    Retrieve Entity
    ...    id=${entity_id}
    ...    accept=${accept}
    Check Response Status Code    ${status_code}    ${response.status_code}
    [Teardown]    Delete Entity    ${entity_id}

049_01_02 Endpoint Get /subscriptions/{subscriptionId}
    [Documentation]    Verify throwing 406 HTTP status code (Not Acceptable Media Type) if the "Accept" header does not imply "application/json" nor "application/ld+json" (get /subscriptions/{subscriptionId})
    [Tags]    sub-retrieve    cb-noacceptable-medtype    6_3_4
    ${id}=    Generate Random Subscription Id
    ${response}=    Retrieve Subscription
    ...    id=${id}
    ...    accept=${accept}
    Check Response Status Code    ${status_code}    ${response.status_code}
    [Teardown]    Delete Subscription    ${id}

049_01_03 Endpoint Get /csourceRegistrations/
    [Documentation]    Verify throwing 406 HTTP status code (Not Acceptable Media Type) if the "Accept" header does not imply "application/json" nor "application/ld+json" (get /csourceRegistrations/)
    [Tags]    csr-query    cb-noacceptable-medtype    6_3_4
    ${response}=    Query Context Source Registrations With Return    type=Building    accept=${accept}
    Check Response Status Code    ${status_code}    ${response.status_code}

049_01_04 Endpoint Get /csourceSubscriptions/
    [Documentation]    Verify throwing 406 HTTP status code (Not Acceptable Media Type) if the "Accept" header does not imply "application/json" nor "application/ld+json" (get /csourceSubscriptions/)
    [Tags]    csrsub-query    cb-noacceptable-medtype    6_3_4
    ${response}=    Query Context Source Registration Subscriptions    accept=${accept}
    Check Response Status Code    ${status_code}    ${response.status_code}

049_01_05 Endpoint Get /temporal/entities
    [Documentation]    Verify throwing 406 HTTP status code (Not Acceptable Media Type) if the "Accept" header does not imply "application/json" nor "application/ld+json" (get /temporal/entities)
    [Tags]    te-query    cb-noacceptable-medtype    6_3_4
    ${entity_types_to_be_retrieved}=    Catenate    SEPARATOR=,    Vehicle
    ${response}=    Query Temporal Representation Of Entities
    ...    entity_types=${entity_types_to_be_retrieved}
    ...    timerel=after
    ...    timeAt=2020-08-01T12:05:00Z
    ...    accept=${accept}
    Check Response Status Code    ${status_code}    ${response.status_code}
